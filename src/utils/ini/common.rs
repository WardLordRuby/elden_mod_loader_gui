use ini::Ini;
use std::{
    collections::HashSet,
    io,
    marker::Sized,
    path::{Path, PathBuf},
};
use tracing::{info, instrument};

use crate::{
    get_or_setup_cfg,
    utils::{
        display::{DisplayTheme, DisplayTime, IntoIoError, ModError},
        ini::{
            parser::{parse_bool, IniProperty},
            writer::{save_bool, save_value_ext, EXT_OPTIONS, WRITE_OPTIONS},
        },
    },
    ARRAY_KEY, ARRAY_VALUE, DEFAULT_INI_VALUES, DEFAULT_LOADER_VALUES, INI_KEYS, INI_NAME,
    INI_SECTIONS, LOADER_FILES, LOADER_KEYS, LOADER_SECTIONS,
};

pub trait Config {
    /// reads a .ini file into memory  
    fn read(ini_dir: &Path) -> io::Result<Self>
    where
        Self: Sized;

    /// returns a reference to where the read file is loacated  
    fn path(&self) -> &Path;

    /// returns a reference to the read in memory data  
    fn data(&self) -> &ini::Ini;

    /// Set (replace) key-value pair in the section (all with the same name)
    fn set(&mut self, section: Option<&str>, key: &str, value: &str);

    /// updates the in memory data of the ini from `self.path()`
    fn update(&mut self) -> io::Result<()>;

    /// manually construct this type, typically use `Config::read()`  
    fn from(data: ini::Ini, ini_dir: &Path) -> Self;

    /// returns a default `Self` with the ini_dir set  
    fn default(ini_dir: &Path) -> Self;

    /// returns a empty `Self`, avoid using `empty()` and use `default()` when possible  
    fn empty() -> Self;

    /// swaps `Self.data` with `Self::default()` and returns you contents
    fn empty_contents(&mut self) -> ini::Ini;

    /// returns `true` if no mods are registered  
    fn mods_is_empty(&self) -> bool;

    /// returns the number of mods registered  
    fn mods_registered(&self) -> usize;

    /// writes the in-memory `self.data()` to the directory stored in `self.path()`
    fn write_to_file(&self) -> io::Result<()>;

    /// saves the computed default value (from key) to to file and appends an error message apon failure  
    fn save_default_val(&self, section: Option<&str>, key: &str, in_err: io::Error) -> io::Error;
}

#[derive(Debug)]
pub struct Cfg {
    data: Ini,
    dir: PathBuf,
}

impl Config for Cfg {
    #[instrument(level = "trace", name = "cfg_read", skip_all)]
    fn read(ini_dir: &Path) -> io::Result<Self>
    where
        Self: Sized,
    {
        Ok(Cfg {
            data: get_or_setup_cfg(ini_dir, &INI_SECTIONS)?,
            dir: PathBuf::from(ini_dir),
        })
    }

    #[inline]
    fn path(&self) -> &Path {
        &self.dir
    }

    #[inline]
    fn data(&self) -> &ini::Ini {
        &self.data
    }

    #[inline]
    fn set(&mut self, section: Option<&str>, key: &str, value: &str) {
        self.data.with_section(section).set(key, value);
    }

    #[inline]
    #[instrument(level = "trace", name = "cfg_update", skip_all)]
    fn update(&mut self) -> io::Result<()> {
        self.data = get_or_setup_cfg(&self.dir, &INI_SECTIONS)?;
        Ok(())
    }

    #[inline]
    fn from(data: ini::Ini, ini_dir: &Path) -> Self {
        Cfg {
            data,
            dir: PathBuf::from(ini_dir),
        }
    }

    #[inline]
    fn default(ini_dir: &Path) -> Self {
        Cfg {
            data: ini::Ini::new(),
            dir: PathBuf::from(ini_dir),
        }
    }

    #[inline]
    fn empty() -> Self {
        Cfg {
            data: ini::Ini::new(),
            dir: PathBuf::new(),
        }
    }

    #[inline]
    fn empty_contents(&mut self) -> ini::Ini {
        std::mem::take(&mut self.data)
    }

    #[inline]
    fn mods_is_empty(&self) -> bool {
        self.data.section(INI_SECTIONS[2]).is_none()
            || self
                .data
                .section(INI_SECTIONS[2])
                .expect("Cfg verified")
                .is_empty()
    }

    fn mods_registered(&self) -> usize {
        if self.mods_is_empty() {
            0
        } else {
            self.data
                .section(INI_SECTIONS[2])
                .expect("Cfg verified")
                .len()
        }
    }

    #[inline]
    fn write_to_file(&self) -> io::Result<()> {
        self.data.write_to_file_opt(&self.dir, WRITE_OPTIONS)
    }

    fn save_default_val(
        &self,
        section: Option<&str>,
        key: &str,
        mut in_err: io::Error,
    ) -> io::Error {
        let default_val = match key {
            k if k == INI_KEYS[0] => DEFAULT_INI_VALUES[0],
            k if k == INI_KEYS[1] => DEFAULT_INI_VALUES[1],
            _ => panic!("Key: {key}, is unknown to: {INI_NAME}"),
        };
        if let Err(err) = save_bool(&self.dir, section, key, default_val) {
            in_err.add_msg(&err.to_string(), false);
        } else {
            in_err.add_msg(&format!("Reset: {key}, to: {default_val}"), false);
        };
        in_err
    }
}

impl Cfg {
    /// returns the value stored with key "dark_mode" as a `bool`  
    /// if error calls `self.save_default_val` to correct error  
    pub fn get_dark_mode(&self) -> io::Result<bool> {
        match IniProperty::<bool>::read(&self.data, INI_SECTIONS[0], INI_KEYS[0]) {
            Ok(dark_mode) => {
                info!("{} theme loaded", DisplayTheme(dark_mode.value));
                Ok(dark_mode.value)
            }
            Err(err) => Err(self.save_default_val(INI_SECTIONS[0], INI_KEYS[0], err)),
        }
    }

    /// returns the value stored with key "save_log" as a `bool`  
    /// if error calls `self.save_default_val` to correct error  
    pub fn get_save_log(&self) -> io::Result<bool> {
        match IniProperty::<bool>::read(&self.data, INI_SECTIONS[0], INI_KEYS[1]) {
            Ok(save_log) => Ok(save_log.value),
            Err(err) => Err(self.save_default_val(INI_SECTIONS[0], INI_KEYS[1], err)),
        }
    }

    /// replaces invalid entries with valid ones and returns a message to display to the user if so  
    /// **Note:** this does not write the validated changes to file
    pub fn validate_entries(&mut self) -> Result<(), Vec<String>> {
        let mut messages = Vec::new();
        let state_len = self.data.section(INI_SECTIONS[2]).map(|d| d.len());
        if let Some(mod_states) = self.data.section_mut(INI_SECTIONS[2]) {
            let remove_keys = mod_states
                .iter_mut()
                .fold(
                    (HashSet::with_capacity(state_len.unwrap()), Vec::new()),
                    |(mut keys, mut keys_to_remove), (k, v)| {
                        if !keys.insert(k) {
                            keys_to_remove.push(k.to_owned());
                        }
                        if let Err(err) = parse_bool(v) {
                            let msg = err.into_io_error(k, v).to_string();
                            info!("{msg}");
                            messages.push(msg);
                            *v = String::from("true");
                        }
                        (keys, keys_to_remove)
                    },
                )
                .1;
            remove_keys.into_iter().for_each(|k| {
                let msg = format!("Duplicate key: {k}, found and removed from: {INI_NAME}");
                info!("{msg}");
                messages.push(msg);
                mod_states.remove(k);
            });
        };
        if let Some(mod_files) = self.data.section_mut(INI_SECTIONS[3]) {
            mod_files.iter_mut().fold("", |mut last_key, (k, v)| {
                if k != ARRAY_KEY {
                    last_key = k;
                }
                if v != ARRAY_VALUE && PathBuf::from(v.clone()).extension().is_none() {
                    let msg = format!("Found invalid file: {v}, saved with key: {last_key}");
                    info!("{msg}");
                    messages.push(msg);
                    v.push_str("path_can_not_point_to.directory");
                }
                last_key
            });
        }
        if !messages.is_empty() {
            return Err(messages);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct ModLoaderCfg {
    data: Ini,
    dir: PathBuf,
}

impl Config for ModLoaderCfg {
    #[instrument(level = "trace", name = "mod_loader_read", skip_all)]
    fn read(ini_dir: &Path) -> io::Result<Self>
    where
        Self: Sized,
    {
        Ok(ModLoaderCfg {
            data: get_or_setup_cfg(ini_dir, &LOADER_SECTIONS)?,
            dir: PathBuf::from(ini_dir),
        })
    }

    #[inline]
    fn path(&self) -> &Path {
        &self.dir
    }

    #[inline]
    fn data(&self) -> &ini::Ini {
        &self.data
    }

    #[inline]
    fn set(&mut self, section: Option<&str>, key: &str, value: &str) {
        self.data.with_section(section).set(key, value);
    }

    #[inline]
    #[instrument(level = "trace", name = "mod_loader_update", skip_all)]
    fn update(&mut self) -> io::Result<()> {
        self.data = get_or_setup_cfg(&self.dir, &LOADER_SECTIONS)?;
        Ok(())
    }

    #[inline]
    fn from(data: ini::Ini, ini_dir: &Path) -> Self {
        ModLoaderCfg {
            data,
            dir: PathBuf::from(ini_dir),
        }
    }

    #[inline]
    fn default(ini_dir: &Path) -> Self {
        ModLoaderCfg {
            data: ini::Ini::new(),
            dir: PathBuf::from(ini_dir),
        }
    }

    #[inline]
    fn empty() -> Self {
        ModLoaderCfg {
            data: ini::Ini::new(),
            dir: PathBuf::new(),
        }
    }

    #[inline]
    fn empty_contents(&mut self) -> ini::Ini {
        std::mem::take(&mut self.data)
    }

    #[inline]
    fn mods_is_empty(&self) -> bool {
        self.data.section(LOADER_SECTIONS[1]).is_none()
            || self
                .data
                .section(LOADER_SECTIONS[1])
                .expect("ModLoader installed and verified")
                .is_empty()
    }

    fn mods_registered(&self) -> usize {
        if self.mods_is_empty() {
            0
        } else {
            self.data
                .section(LOADER_SECTIONS[1])
                .expect("ModLoader installed and verified")
                .len()
        }
    }

    #[inline]
    fn write_to_file(&self) -> io::Result<()> {
        self.data.write_to_file_opt(&self.dir, EXT_OPTIONS)
    }

    fn save_default_val(
        &self,
        section: Option<&str>,
        key: &str,
        mut in_err: io::Error,
    ) -> io::Error {
        let default_val = match key {
            k if k == LOADER_KEYS[0] => DEFAULT_LOADER_VALUES[0],
            k if k == LOADER_KEYS[1] => DEFAULT_LOADER_VALUES[1],
            _ => panic!("Key: {key}, is unknown to: {}", LOADER_FILES[3]),
        };
        if let Err(err) = save_value_ext(&self.dir, section, key, default_val) {
            in_err.add_msg(&err.to_string(), false);
        } else {
            in_err.add_msg(&format!("Reset: {key}, to: {default_val}"), false);
        };
        in_err
    }
}

impl ModLoaderCfg {
    /// returns value stored with key "load_delay" as `u32`  
    /// if error calls `self.save_default_val` to correct error  
    pub fn get_load_delay(&self) -> io::Result<u32> {
        match IniProperty::<u32>::read(&self.data, LOADER_SECTIONS[0], LOADER_KEYS[0]) {
            Ok(delay_time) => {
                info!("Load delay: {}", DisplayTime(delay_time.value));
                Ok(delay_time.value)
            }
            Err(err) => Err(self.save_default_val(LOADER_SECTIONS[0], LOADER_KEYS[0], err)),
        }
    }

    /// returns value stored with key "show_terminal" as `bool`  
    /// if error calls `self.save_default_val` to correct error  
    pub fn get_show_terminal(&self) -> io::Result<bool> {
        match IniProperty::<bool>::read(&self.data, LOADER_SECTIONS[0], LOADER_KEYS[1]) {
            Ok(show_terminal) => {
                info!("Show terminal: {}", show_terminal.value);
                Ok(show_terminal.value)
            }
            Err(err) => Err(self.save_default_val(LOADER_SECTIONS[0], LOADER_KEYS[1], err)),
        }
    }

    /// retuns mutable reference to key value pairs stored in "loadorder"  
    #[inline]
    pub fn mut_section(&mut self) -> &mut ini::Properties {
        self.data
            .section_mut(LOADER_SECTIONS[1])
            .expect("ModLoader installed and verified")
    }

    /// retuns immutable reference to key value pairs stored in "loadorder"  
    #[inline]
    pub fn section(&self) -> &ini::Properties {
        self.data
            .section(LOADER_SECTIONS[1])
            .expect("ModLoader installed and verified")
    }

    /// get an iterator of the key value pairs stored in "loadorder"  
    #[inline]
    pub fn iter(&self) -> ini::PropertyIter {
        self.section().iter()
    }
}

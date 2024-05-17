use ini::Ini;
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};

use crate::{
    get_or_setup_cfg,
    utils::ini::{
        parser::{IniProperty, ModError},
        writer::{save_bool, save_value_ext, EXT_OPTIONS, WRITE_OPTIONS},
    },
    DEFAULT_INI_VALUES, DEFAULT_LOADER_VALUES, INI_KEYS, INI_SECTIONS, LOADER_KEYS,
    LOADER_SECTIONS,
};

pub trait Config {
    fn read(ini_path: &Path) -> std::io::Result<Self>
    where
        Self: std::marker::Sized;

    fn path(&self) -> &Path;

    fn data(&self) -> &ini::Ini;

    fn set(&mut self, section: Option<&str>, key: &str, value: &str);

    fn update(&mut self) -> std::io::Result<()>;

    fn from(data: ini::Ini, dir: &Path) -> Self;

    fn default(dir: &Path) -> Self;

    fn empty() -> Self;

    fn write_to_file(&self) -> std::io::Result<()>;

    #[allow(unused_variables)]
    #[allow(unused_mut)]
    fn save_default_val(
        &self,
        section: Option<&str>,
        key: &str,
        mut in_err: std::io::Error,
    ) -> std::io::Error {
        std::io::Error::new(
            ErrorKind::WriteZero,
            "Please implement `save_default_val()` for your type",
        )
    }
}

#[derive(Debug)]
pub struct Cfg {
    data: Ini,
    dir: PathBuf,
}

impl Config for Cfg {
    fn read(ini_path: &Path) -> std::io::Result<Self>
    where
        Self: std::marker::Sized,
    {
        let data = get_or_setup_cfg(ini_path, &INI_SECTIONS)?;
        Ok(Cfg {
            data,
            dir: PathBuf::from(ini_path),
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

    fn update(&mut self) -> std::io::Result<()> {
        self.data = get_or_setup_cfg(&self.dir, &INI_SECTIONS)?;
        Ok(())
    }

    fn from(data: ini::Ini, dir: &Path) -> Self {
        Cfg {
            data,
            dir: PathBuf::from(dir),
        }
    }

    #[inline]
    fn default(dir: &Path) -> Self {
        Cfg {
            data: ini::Ini::new(),
            dir: PathBuf::from(dir),
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
    fn write_to_file(&self) -> std::io::Result<()> {
        self.data.write_to_file_opt(&self.dir, WRITE_OPTIONS)
    }

    fn save_default_val(
        &self,
        section: Option<&str>,
        key: &str,
        mut in_err: std::io::Error,
    ) -> std::io::Error {
        save_bool(&self.dir, section, key, DEFAULT_INI_VALUES[0]).unwrap_or_else(|err| {
            in_err.add_msg(&format!("\n, {err}"));
            // io::write error
        });
        in_err
    }
}

impl Cfg {
    pub fn get_dark_mode(&self) -> std::io::Result<bool> {
        match IniProperty::<bool>::read(&self.data, INI_SECTIONS[0], INI_KEYS[0]) {
            Ok(dark_mode) => Ok(dark_mode.value),
            Err(mut err) => {
                err.add_msg(&format!(
                    "Found an unexpected character saved in \"{}\". Reseting to default value",
                    LOADER_KEYS[0]
                ));
                Err(self.save_default_val(INI_SECTIONS[0], INI_KEYS[0], err))
            }
        }
    }

    /// returns the number of registered mods currently saved in the ".ini"  
    pub fn mods_registered(&self) -> usize {
        if self.data.section(INI_SECTIONS[2]).is_none()
            || self.data.section(INI_SECTIONS[2]).unwrap().is_empty()
        {
            0
        } else {
            self.data.section(INI_SECTIONS[2]).unwrap().len()
        }
    }

    /// returns true if registered mods saved in the ".ini" is None  
    #[inline]
    pub fn mods_empty(&self) -> bool {
        self.data.section(INI_SECTIONS[2]).is_none()
            || self.data.section(INI_SECTIONS[2]).unwrap().is_empty()
    }
}

#[derive(Debug)]
pub struct ModLoaderCfg {
    data: Ini,
    dir: PathBuf,
}

impl Config for ModLoaderCfg {
    fn read(ini_path: &Path) -> std::io::Result<Self>
    where
        Self: std::marker::Sized,
    {
        let data = get_or_setup_cfg(ini_path, &LOADER_SECTIONS)?;
        Ok(ModLoaderCfg {
            data,
            dir: PathBuf::from(ini_path),
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

    fn update(&mut self) -> std::io::Result<()> {
        self.data = get_or_setup_cfg(&self.dir, &LOADER_SECTIONS)?;
        Ok(())
    }

    fn from(data: ini::Ini, dir: &Path) -> Self {
        ModLoaderCfg {
            data,
            dir: PathBuf::from(dir),
        }
    }

    #[inline]
    fn default(dir: &Path) -> Self {
        ModLoaderCfg {
            data: ini::Ini::new(),
            dir: PathBuf::from(dir),
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
    fn write_to_file(&self) -> std::io::Result<()> {
        self.data.write_to_file_opt(&self.dir, EXT_OPTIONS)
    }

    fn save_default_val(
        &self,
        section: Option<&str>,
        key: &str,
        mut in_err: std::io::Error,
    ) -> std::io::Error {
        let default_val = match key {
            k if k == LOADER_KEYS[0] => DEFAULT_LOADER_VALUES[0],
            k if k == LOADER_KEYS[1] => DEFAULT_LOADER_VALUES[1],
            _ => panic!("Unknown key was passed in"),
        };
        save_value_ext(&self.dir, section, key, default_val).unwrap_or_else(|err| {
            in_err.add_msg(&format!("\n, {err}"));
            // io::write error
        });
        in_err
    }
}

impl ModLoaderCfg {
    pub fn get_load_delay(&self) -> std::io::Result<u32> {
        match IniProperty::<u32>::read(&self.data, LOADER_SECTIONS[0], LOADER_KEYS[0]) {
            Ok(delay_time) => Ok(delay_time.value),
            Err(mut err) => {
                err.add_msg(&format!(
                    "Found an unexpected character saved in \"{}\". Reseting to default value",
                    LOADER_KEYS[0]
                ));
                Err(self.save_default_val(LOADER_SECTIONS[0], LOADER_KEYS[0], err))
            }
        }
    }

    pub fn get_show_terminal(&self) -> std::io::Result<bool> {
        match IniProperty::<bool>::read(&self.data, LOADER_SECTIONS[0], LOADER_KEYS[1]) {
            Ok(bool) => Ok(bool.value),
            Err(mut err) => {
                err.add_msg(&format!(
                    "Found an unexpected character saved in \"{}\". Reseting to default value",
                    LOADER_KEYS[1]
                ));
                Err(self.save_default_val(LOADER_SECTIONS[0], LOADER_KEYS[1], err))
            }
        }
    }

    #[inline]
    pub fn mut_section(&mut self) -> &mut ini::Properties {
        self.data.section_mut(LOADER_SECTIONS[1]).unwrap()
    }

    #[inline]
    pub fn section(&self) -> &ini::Properties {
        self.data.section(LOADER_SECTIONS[1]).unwrap()
    }

    #[inline]
    pub fn iter(&self) -> ini::PropertyIter {
        self.section().iter()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.section().is_empty()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.section().len()
    }
}

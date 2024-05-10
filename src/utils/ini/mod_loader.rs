use ini::Ini;
use log::{trace, warn};
use std::{
    collections::{HashMap, HashSet},
    io::ErrorKind,
    path::{Path, PathBuf},
};

use crate::{
    does_dir_contain, get_or_setup_cfg, new_io_error,
    utils::ini::{
        parser::{IniProperty, ModError, RegMod},
        writer::{new_cfg, EXT_OPTIONS},
    },
    Operation, OperationResult, LOADER_FILES, LOADER_KEYS, LOADER_SECTIONS,
};

#[derive(Debug, Default)]
pub struct ModLoader {
    installed: bool,
    disabled: bool,
    path: PathBuf,
}

impl ModLoader {
    pub fn properties(game_dir: &Path) -> std::io::Result<ModLoader> {
        let mut cfg_dir = game_dir.join(LOADER_FILES[2]);
        let mut properties = ModLoader::default();
        match does_dir_contain(game_dir, Operation::Count, &LOADER_FILES) {
            Ok(OperationResult::Count((_, files))) => {
                if files.contains(LOADER_FILES[1]) && !files.contains(LOADER_FILES[0]) {
                    trace!("Mod loader found in the Enabled state");
                    properties.installed = true;
                } else if files.contains(LOADER_FILES[0]) && !files.contains(LOADER_FILES[1]) {
                    trace!("Mod loader found in the Disabled state");
                    properties.installed = true;
                    properties.disabled = true;
                }
                if files.contains(LOADER_FILES[2]) {
                    std::mem::swap(&mut cfg_dir, &mut properties.path);
                }
            }
            Err(err) => return Err(err),
            _ => unreachable!(),
        };
        if properties.installed && properties.path == Path::new("") {
            trace!("{} not found, creating new", LOADER_FILES[2]);
            new_cfg(&cfg_dir)?;
            properties.path = cfg_dir;
        }
        if !properties.installed {
            warn!("Mod loader dll hook not found");
        }
        Ok(properties)
    }

    #[inline]
    pub fn installed(&self) -> bool {
        self.installed
    }

    #[inline]
    pub fn disabled(&self) -> bool {
        self.disabled
    }

    #[inline]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[inline]
    pub fn own_path(self) -> PathBuf {
        self.path
    }
}

#[derive(Debug)]
pub struct ModLoaderCfg {
    data: Ini,
    dir: PathBuf,
    section: Option<String>,
}

impl ModLoaderCfg {
    pub fn read_section(cfg_dir: &Path, section: Option<&str>) -> std::io::Result<ModLoaderCfg> {
        if section.is_none() {
            return new_io_error!(ErrorKind::InvalidInput, "section can not be none");
        }

        let data = get_or_setup_cfg(cfg_dir, &LOADER_SECTIONS)?;
        Ok(ModLoaderCfg {
            data,
            dir: PathBuf::from(cfg_dir),
            section: section.map(String::from),
        })
    }

    pub fn get_load_delay(&self) -> std::io::Result<u32> {
        match IniProperty::<u32>::read(&self.data, LOADER_SECTIONS[0], LOADER_KEYS[0]) {
            Ok(delay_time) => Ok(delay_time.value),
            Err(err) => Err(err.add_msg(format!(
                "Found an unexpected character saved in \"{}\"",
                LOADER_KEYS[0]
            ))),
        }
    }

    pub fn get_show_terminal(&self) -> std::io::Result<bool> {
        match IniProperty::<bool>::read(&self.data, LOADER_SECTIONS[0], LOADER_KEYS[1]) {
            Ok(delay_time) => Ok(delay_time.value),
            Err(err) => Err(err.add_msg(format!(
                "Found an unexpected character saved in \"{}\"",
                LOADER_KEYS[1]
            ))),
        }
    }

    #[inline]
    pub fn mut_section(&mut self) -> &mut ini::Properties {
        self.data.section_mut(self.section.as_ref()).unwrap()
    }

    #[inline]
    fn section(&self) -> &ini::Properties {
        self.data.section(self.section.as_ref()).unwrap()
    }

    #[inline]
    /// updates the current section, general sections `None` are not supported
    pub fn set_section(&mut self, new: Option<&str>) {
        if new.is_some() {
            self.section = new.map(String::from)
        }
    }

    #[inline]
    pub fn iter(&self) -> ini::PropertyIter {
        self.section().iter()
    }

    pub fn verify_keys(&mut self, mods: &[RegMod]) -> std::io::Result<()> {
        let valid_dlls = mods
            .iter()
            .flat_map(|m| {
                m.files
                    .dll
                    .iter()
                    .filter_map(|f| f.file_name()?.to_str())
                    .collect::<Vec<_>>()
            })
            .collect::<HashSet<_>>();
        let order_count = mods.order_count();
        let keys = self.iter().map(|(k, _)| k.to_string()).collect::<Vec<_>>();
        let mut unknown_keys = Vec::new();
        let mut update_order = false;
        keys.iter().enumerate().for_each(|(i, k)| {
            if !valid_dlls.contains(k.as_str()) {
                unknown_keys.push(k.to_owned());
                if i < order_count {
                    update_order = true; 
                    self.mut_section().remove(k);
                    self.mut_section().append(k, "69420");
                }
            }
        });
        if !unknown_keys.is_empty() {
            if update_order {
                self.update_order_entries(None)?;
                return new_io_error!(ErrorKind::Unsupported, 
                    format!("Found load order set for files not registered with the app. The following key(s) order were changed {}", 
                    unknown_keys.join("\n"))
                );
            }
            return new_io_error!(ErrorKind::Other,
                format!("Found load order set for the following files not registered with the app. {}", 
                unknown_keys.join("\n"))
            );
        }
        Ok(())
    }

    /// returns an owned `HashMap` with values parsed into K: `String`, V: `usize`  
    /// this function also fixes usize.parse() errors and if values are out of order
    pub fn parse_section(&mut self) -> std::io::Result<HashMap<String, usize>> {
        let map = self.parse_into_map();
        if self.section().len() != map.len() {
            trace!("fixing usize parse error in \"{}\"", LOADER_FILES[2]);
            self.update_order_entries(None)?;
            return Ok(self.parse_into_map());
        }
        let mut values = self.iter().filter_map(|(k, _)| map.get(k)).collect::<Vec<_>>();
        values.sort();
        for (i, value) in values.iter().enumerate() {
            if i != **value {
                trace!(
                    "values in \"{}\" are not in order, sorting entries",
                    LOADER_FILES[2]
                );
                self.update_order_entries(None)?;
                return Ok(self.parse_into_map());
            }
        }
        Ok(map)
    }

    /// returns an owned `HashMap` with values parsed into K: `String`, V: `usize`  
    /// this will filter out invalid entries, do not use unless you _know_ all entries are valid  
    pub fn parse_into_map(&self) -> HashMap<String, usize> {
        self.iter()
            .filter_map(|(k, v)| Some((k.to_string(), v.parse::<usize>().ok()?)))
            .collect::<HashMap<String, usize>>()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.section().is_empty()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.section().len()
    }

    #[inline]
    pub fn path(&self) -> &Path {
        &self.dir
    }

    pub fn write_to_file(&self) -> std::io::Result<()> {
        self.data.write_to_file_opt(&self.dir, EXT_OPTIONS)
    }
    // MARK: TODO
    // we need to account for unexpected orders that are set
    // we either need to move them to the end or remove them from the section

    /// updates the load order values in `Some("loadorder")` so they are always `0..`  
    /// if you want a key's value to remain the unedited you can supply `Some(stable_key)`  
    /// then writes the updated key values to file
    ///
    /// error cases:  
    ///     section is not set to "loadorder"  
    ///     fails to write to file  
    pub fn update_order_entries(&mut self, stable: Option<&str>) -> std::io::Result<()> {
        if self.section.as_deref() != LOADER_SECTIONS[1] {
            return new_io_error!(
                ErrorKind::InvalidInput,
                format!(
                    "This function is only supported to modify Section: \"{}\"",
                    LOADER_SECTIONS[1].unwrap()
                )
            );
        }
        let mut k_v = Vec::with_capacity(self.section().len());
        let (mut stable_k, mut stable_v) = ("", 0_usize);
        for (k, v) in self.iter() {
            if let Some(new_k) = stable {
                if k == new_k {
                    (stable_k, stable_v) = (k, v.parse::<usize>().unwrap_or(usize::MAX));
                    continue;
                }
            }
            k_v.push((k, v.parse::<usize>().unwrap_or(usize::MAX)));
        }
        k_v.sort_by_key(|(_, v)| *v);

        let mut new_section = ini::Properties::new();

        if k_v.is_empty() && !stable_k.is_empty() {
            new_section.append(stable_k, "0");
        } else {
            let mut offset = 0_usize;
            for (k, _) in k_v {
                if !stable_k.is_empty() && stable_v == offset {
                    new_section.append(std::mem::take(&mut stable_k), stable_v.to_string());
                    offset += 1;
                }
                new_section.append(k, offset.to_string());
                offset += 1;
            }
            if !stable_k.is_empty() {
                new_section.append(stable_k, offset.to_string())
            }
        }
        std::mem::swap(self.mut_section(), &mut new_section);
        self.write_to_file()
    }

    pub fn default(path: &Path) -> Self {
        ModLoaderCfg {
            data: ini::Ini::new(),
            dir: PathBuf::from(path),
            section: None,
        }
    }

    pub fn empty() -> Self {
        ModLoaderCfg {
            data: ini::Ini::new(),
            dir: PathBuf::new(),
            section: None,
        }
    }
}

pub trait Countable {
    fn order_count(&self) -> usize;
}

impl Countable for &[RegMod] {
    #[inline]
    fn order_count(&self) -> usize {
        self.iter().filter(|m| m.order.set).count()
    }
}

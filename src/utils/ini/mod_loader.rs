use std::{
    collections::HashSet,
    io::ErrorKind,
    path::{Path, PathBuf},
};
use tracing::{info, instrument, trace, warn};

use crate::{
    does_dir_contain, new_io_error, omit_off_state,
    utils::ini::{
        common::{Config, ModLoaderCfg},
        parser::RegMod,
        writer::new_cfg,
    },
    DisplayState, Operation, OperationResult, OrderMap, LOADER_FILES,
};

#[derive(Debug, Default)]
pub struct ModLoader {
    installed: bool,
    disabled: bool,
    path: PathBuf,
}

impl ModLoader {
    /// returns struct `ModLoader` that contains properties about the current installation of  
    /// the _elden_mod_loader_ dll hook by TechieW
    ///
    /// can only error if it finds loader hook installed && "elden_mod_loader_config.ini" is not found so it fails on writing a new one to disk
    #[instrument(level = "trace", name = "mod_loader_properties", skip_all)]
    pub fn properties(game_dir: &Path) -> std::io::Result<ModLoader> {
        let mut cfg_dir = game_dir.join(LOADER_FILES[2]);
        let mut properties = ModLoader::default();
        match does_dir_contain(game_dir, Operation::Count, &LOADER_FILES) {
            Ok(OperationResult::Count((_, files))) => {
                if files.contains(LOADER_FILES[1]) && !files.contains(LOADER_FILES[0]) {
                    properties.installed = true;
                } else if files.contains(LOADER_FILES[0]) && !files.contains(LOADER_FILES[1]) {
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
            info!("{} not found", LOADER_FILES[2]);
            new_cfg(&cfg_dir)?;
            properties.path = cfg_dir;
        }
        if !properties.installed {
            warn!("Mod loader dll hook: {}, not found", LOADER_FILES[1]);
        } else {
            trace!(dll_hook = %DisplayState(!properties.disabled), "elden_mod_loader files found");
        }
        Ok(properties)
    }

    /// only use this if `ModLoader::properties()` returns err and you have an idea of the current state
    pub fn new(disabled: bool) -> Self {
        ModLoader {
            installed: true,
            disabled,
            path: PathBuf::new(),
        }
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

impl ModLoaderCfg {
    /// verifies that all keys stored in "elden_mod_loader_config.ini" are registered with the app  
    /// a _unknown_ file is found as a key this will change the order to be greater than _known_ files  
    /// a `DllSet` is obtained by calling `dll_name_set()` on a `[RegMod]`  
    #[instrument(level = "trace", skip_all)]
    pub fn verify_keys(&mut self, dlls: &DllSet, order_count: usize) -> std::io::Result<()> {
        let keys = self.iter().map(|(k, _)| k.to_string()).collect::<Vec<_>>();
        let mut unknown_keys = Vec::new();
        let mut update_order = false;
        keys.iter().enumerate().for_each(|(i, k)| {
            if !dlls.contains(k.as_str()) {
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
                    format!("Found load order set for files not registered with the app. The following key(s) order were changed: {}", 
                    unknown_keys.join(", "))
                );
            }
            return new_io_error!(
                ErrorKind::Other,
                format!(
                    "Found load order set for the following files not registered with the app: {}",
                    unknown_keys.join(", ")
                )
            );
        }
        trace!("all load_order entries are files registered with the app");
        Ok(())
    }

    /// returns an owned `HashMap` with values parsed into K: `String`, V: `usize`  
    /// this function also fixes usize.parse() errors and if values are out of order
    #[instrument(level = "trace", skip_all)]
    pub fn parse_section(&mut self) -> std::io::Result<OrderMap> {
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
    /// this will not filter out invalid entries, do not use unless you _know_ all entries are valid  
    pub fn parse_into_map(&self) -> OrderMap {
        self.iter()
            .filter_map(|(k, v)| Some((k.to_string(), v.parse::<usize>().ok()?)))
            .collect::<OrderMap>()
    }

    /// updates the load order values in `Some("loadorder")` so they are always `0..`  
    /// if you want a key's value to remain the unedited you can supply `Some(stable_key)`  
    /// then writes the updated key values to file
    ///
    /// error cases:
    /// - section is not set to "loadorder"  
    /// - fails to write to file  
    #[instrument(level = "trace", skip(self))]
    pub fn update_order_entries(&mut self, stable: Option<&str>) -> std::io::Result<()> {
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
        trace!("re-calculated the order of entries in {}", LOADER_FILES[2]);
        self.write_to_file()
    }
}

pub trait Countable {
    /// returns the number of entries in a colletion that have `mod.order.set`
    fn order_count(&self) -> usize;
}

impl Countable for [RegMod] {
    #[inline]
    fn order_count(&self) -> usize {
        self.iter().filter(|m| m.order.set).count()
    }
}

type DllSet<'a> = HashSet<&'a str>;
pub trait NameSet {
    fn dll_name_set(&self) -> HashSet<&str>;
}

impl NameSet for [RegMod] {
    fn dll_name_set(&self) -> DllSet {
        self.iter()
            .flat_map(|reg_mod| {
                reg_mod
                    .files
                    .dll
                    .iter()
                    .filter_map(|f| Some(omit_off_state(f.file_name()?.to_str()?)))
                    .collect::<Vec<_>>()
            })
            .collect::<HashSet<_>>()
    }
}

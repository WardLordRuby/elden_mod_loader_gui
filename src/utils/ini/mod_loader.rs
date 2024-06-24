use std::{
    collections::{HashMap, HashSet},
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
    DisplayState, DisplayVec, Operation, OperationResult, OrderMap, LOADER_EXAMPLE, LOADER_FILES,
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
            // MARK: TODO
            // add state for if _dinput8.dll is found (how anti-cheat-toggle will disable mod loader)
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
        if properties.installed && properties.path.as_os_str().is_empty() {
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
        if self.mods_is_empty() {
            trace!("No mods have load order");
            return Ok(());
        }
        let keys = self
            .iter()
            .filter_map(|(k, _)| {
                if k != LOADER_EXAMPLE {
                    Some(k.to_string())
                } else {
                    trace!("{LOADER_EXAMPLE} ignored");
                    None
                }
            })
            .collect::<Vec<_>>();
        if keys.is_empty() {
            return Ok(());
        }
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
                self.update_order_entries(None);
                self.write_to_file()?;
                return new_io_error!(ErrorKind::Unsupported,
                    format!("Found load order set for file(s) not registered with the app. The following key(s) order were changed: {}", 
                    DisplayVec(&unknown_keys))
                );
            }
            return new_io_error!(
                ErrorKind::Other,
                format!(
                    "Found load order set for the following file(s) not registered with the app: {}",
                    DisplayVec(&unknown_keys)
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
        if self.section().contains_key(LOADER_EXAMPLE) {
            self.mut_section().remove(LOADER_EXAMPLE);
            self.write_to_file()?;
            info!("Removed: '{LOADER_EXAMPLE}' from: {}", LOADER_FILES[2]);
        }
        if self.mods_is_empty() {
            trace!("No mods have load order");
            return Ok(HashMap::new());
        }
        let map = self.parse_into_map();
        if self.mods_registered() != map.len() {
            trace!("fixing usize parse error in: {}", LOADER_FILES[2]);
            self.update_order_entries(None);
            self.write_to_file()?;
            return Ok(self.parse_into_map());
        }
        let mut values = self.iter().filter_map(|(k, _)| map.get(k)).collect::<Vec<_>>();
        values.sort();
        let mut count = if *values[0] == 0 { 0 } else { 1 };
        for value in values {
            if count != *value && {
                count += 1;
                count
            } != *value
            {
                self.update_order_entries(None);
                self.write_to_file()?;
                info!(
                    "Found entries out of order, sorted load order entries in: {}",
                    LOADER_FILES[2]
                );
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

    /// updates the load order values in `Some("loadorder")` so they are always no gaps in values  
    /// if you want a key's value to remain the unedited you can supply `Some(stable_key)`  
    /// then returns returns the calculation for the correct max_order val (same logic appears in `[RegMod].max_order()`)
    #[instrument(level = "trace", skip(self))]
    pub fn update_order_entries(&mut self, stable: Option<&str>) -> usize {
        if self.mods_is_empty() && stable.is_none() {
            trace!("nothing to update");
            return 1;
        }
        let mut k_v = Vec::with_capacity(self.section().len());
        let (mut stable_k, mut stable_v) = ("", 69420_usize);
        for (k, v) in self.iter() {
            if let Some(input_k) = stable {
                if k == input_k {
                    (stable_k, stable_v) = (k, v.parse::<usize>().unwrap_or(usize::MAX));
                    continue;
                }
            }
            k_v.push((k, v.parse::<usize>().unwrap_or(usize::MAX)));
        }
        k_v.sort_by_key(|(_, v)| *v);
        dbg!(&k_v);
        dbg!((stable_k, stable_v));

        let mut new_section = ini::Properties::new();

        let max_order = if k_v.is_empty() && !stable_k.is_empty() {
            new_section.append(stable_k, if stable_v == 0 { "0" } else { "1" });
            1
        } else {
            let mut offset: usize = if (!k_v.is_empty() && k_v[0].1 == 0) || stable_v == 0 {
                0
            } else {
                1
            };
            let mut iter = k_v.iter().peekable();
            while let Some((k, v)) = iter.next() {
                if !stable_k.is_empty() && stable_v == offset {
                    new_section.append(std::mem::take(&mut stable_k), offset.to_string());
                    if *v != stable_v && *v > offset {
                        offset += 1;
                    }
                }
                new_section.append(*k, offset.to_string());
                if let Some((_, next_v)) = iter.peek() {
                    if v != next_v && *next_v > offset {
                        offset += 1;
                    }
                } else if !stable_k.is_empty() && *v != stable_v && stable_v > offset {
                    offset += 1;
                }
            }
            let end_offset_str = offset.to_string();
            if !stable_k.is_empty() {
                new_section.append(stable_k, &end_offset_str);
            }
            if new_section.len() == 1 {
                1
            } else if new_section.iter().filter(|(_, v)| *v == end_offset_str).count() == 1 {
                offset
            } else {
                offset + 1
            }
        };
        dbg!(&new_section);
        std::mem::swap(self.mut_section(), &mut new_section);
        trace!("re-calculated the order of entries in {}", LOADER_FILES[2]);
        max_order
    }
}

type DllSet<'a> = HashSet<&'a str>;

pub trait RegModsExt {
    /// returns the number of entries in a colletion that have `mod.order.set`
    fn order_count(&self) -> usize;

    /// returns the calculation for the correct max_order val
    fn max_order(&self) -> usize;

    /// returns a `HashSet` of all .dll files with their `OFFSTATE` omitted
    fn dll_name_set(&self) -> DllSet;
}

impl RegModsExt for [RegMod] {
    #[inline]
    fn order_count(&self) -> usize {
        self.iter().filter(|m| m.order.set).count()
    }

    fn max_order(&self) -> usize {
        let set_indices = self
            .iter()
            .enumerate()
            .filter(|(_, m)| m.order.set)
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        if set_indices.is_empty() || set_indices.len() == 1 {
            return 1;
        }
        let high_order = set_indices
            .iter()
            .map(|i| self[*i].order.at)
            .max()
            .expect("order set to a usize");
        if set_indices
            .iter()
            .filter(|i| self[**i].order.at == high_order)
            .count()
            == 1
        {
            high_order
        } else {
            high_order + 1
        }
    }

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

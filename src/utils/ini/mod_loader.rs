use std::{
    collections::{HashMap, HashSet},
    io::ErrorKind,
    path::{Path, PathBuf},
};
use tracing::{info, instrument, trace, warn};

use crate::{
    does_dir_contain,
    utils::ini::{
        common::{Config, ModLoaderCfg},
        parser::RegMod,
        writer::new_cfg,
    },
    DisplayState, DisplayVec, DllSet, Operation, OperationResult, OrderMap, ANTI_CHEAT_EXE,
    LOADER_EXAMPLE, LOADER_FILES,
};

#[derive(Debug, Default)]
pub struct ModLoader {
    installed: bool,
    disabled: bool,
    anti_cheat_toggle_installed: bool,
    anti_cheat_enabled: bool,
    path: PathBuf,
}

impl ModLoader {
    /// returns struct `ModLoader` that contains properties about the current installation of  
    /// the _elden_mod_loader_ dll hook by TechieW
    ///
    /// can only error if it finds loader hook installed && "elden_mod_loader_config.ini" is not found so it fails on writing a new one to disk
    #[instrument(level = "trace", name = "mod_loader_properties", skip_all)]
    pub fn properties(game_dir: &Path) -> std::io::Result<ModLoader> {
        let mut cfg_dir = game_dir.join(LOADER_FILES[3]);
        let mut properties = ModLoader::default();
        let search_for = LOADER_FILES
            .iter()
            .copied()
            .chain(std::iter::once(ANTI_CHEAT_EXE))
            .collect::<Vec<_>>();
        match does_dir_contain(game_dir, Operation::Count, &search_for) {
            Ok(OperationResult::Count((_, files))) => {
                if files.contains(LOADER_FILES[1])
                    && !files.contains(LOADER_FILES[0])
                    && !files.contains(LOADER_FILES[2])
                {
                    properties.installed = true;
                } else if files.contains(LOADER_FILES[0])
                    && !files.contains(LOADER_FILES[1])
                    && !files.contains(LOADER_FILES[2])
                {
                    properties.installed = true;
                    properties.disabled = true;
                } else if files.contains(LOADER_FILES[2])
                    && !files.contains(LOADER_FILES[1])
                    && !files.contains(LOADER_FILES[0])
                {
                    properties.installed = true;
                    properties.disabled = true;
                    properties.anti_cheat_enabled = true;
                }
                if files.contains(ANTI_CHEAT_EXE) {
                    properties.anti_cheat_toggle_installed = true;
                }
                if properties.anti_cheat_enabled && !properties.anti_cheat_toggle_installed {
                    std::fs::rename(
                        game_dir.join(LOADER_FILES[2]),
                        game_dir.join(LOADER_FILES[0]),
                    )?;
                    info!("Renamed: {}, to: {}", LOADER_FILES[2], LOADER_FILES[0]);
                    properties.anti_cheat_enabled = false;
                }
                if files.contains(LOADER_FILES[3]) {
                    std::mem::swap(&mut cfg_dir, &mut properties.path);
                }
            }
            Err(err) => return Err(err),
            _ => unreachable!(),
        };
        if properties.installed && properties.path.as_os_str().is_empty() {
            info!("{} not found", LOADER_FILES[3]);
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
            anti_cheat_toggle_installed: false,
            anti_cheat_enabled: false,
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
    pub fn anti_cheat_toggle_installed(&self) -> bool {
        self.anti_cheat_toggle_installed
    }

    #[inline]
    pub fn anti_cheat_enabled(&self) -> bool {
        self.anti_cheat_enabled
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

/// it is save to update the global `UNKNOWN_ORDER_KEYS` with `unknown_keys` if `is_some()`  
/// this is because of the case a write to file fails `unknown_keys` will be `None`
pub struct UnknownKeyErr {
    pub err: std::io::Error,
    pub unknown_keys: Option<HashSet<String>>,
    pub update_ord_data: Option<OrdMetaData>,
}

impl UnknownKeyErr {
    fn empty_with_err(err: std::io::Error) -> Self {
        UnknownKeyErr {
            err,
            unknown_keys: None,
            update_ord_data: None,
        }
    }
}

pub struct OrdMetaData {
    /// (`max_order`, `high_val.count() > 1`)
    pub max_order: (usize, bool),
    pub missing_vals: Option<Vec<usize>>,
}

impl OrdMetaData {
    pub fn with_ord(max_order: (usize, bool)) -> Self {
        OrdMetaData {
            max_order,
            missing_vals: None,
        }
    }
}

impl ModLoaderCfg {
    /// verifies that all keys stored in "elden_mod_loader_config.ini" are registered with the app  
    /// a _unknown_ file is found as a key this will change the order to be greater than _known_ files  
    /// `DllSet` and `order_count` are retrieved by calling `dll_set_order_count` on `Cfg`  
    ///
    /// **Note:** if `UnknownKeyErr.err.kind() == Unsupported` then  
    /// `update_order_entries()` & `self.write_to_file()` are called  
    /// as a result `OrdMetaData` is re-calculated and returned
    #[instrument(level = "trace", skip_all)]
    pub fn verify_keys(&mut self, dlls: &DllSet, order_count: usize) -> Result<(), UnknownKeyErr> {
        if self.mods_is_empty() {
            trace!("No mods have load order");
            return Ok(());
        }
        let mut high_order = None;
        let mut unknown_keys = Vec::new();
        let mut unknown_vals = Vec::new();
        for (k, v) in self.iter() {
            if k == LOADER_EXAMPLE {
                trace!("{LOADER_EXAMPLE} ignored");
                continue;
            }
            let curr_v = v.parse::<usize>().unwrap_or(42069);
            if dlls.contains(k) {
                if curr_v != 42069 {
                    if let Some(ref mut prev_high) = high_order {
                        if curr_v > *prev_high {
                            *prev_high = curr_v;
                        }
                    } else {
                        high_order = Some(curr_v);
                    }
                }
            } else {
                unknown_keys.push(k.to_string());
                unknown_vals.push(curr_v);
            }
        }
        if unknown_keys.is_empty() {
            trace!("all load_order entries are files registered with the app");
            return Ok(());
        }
        let mut update_order = false;
        let mut update_entry = |k: &String, v: usize| {
            update_order = true;
            self.mut_section().insert(k, (usize::MAX - v).to_string());
        };
        let mut no_user_vals_counter = 0_usize;
        unknown_keys.iter().zip(unknown_vals).for_each(|(k, v)| {
            if order_count == 0 {
                if v != no_user_vals_counter && {
                    no_user_vals_counter += 1;
                    no_user_vals_counter
                } != v
                {
                    update_entry(k, v);
                }
            } else if let Some(ref high_order) = high_order {
                if v <= *high_order {
                    update_entry(k, v);
                }
            } else {
                update_entry(k, v);
            }
        });
        if update_order {
            let err = std::io::Error::new(ErrorKind::Unsupported,
                    format!("Found load order set for file(s) not registered with the app. One or more of the following key(s) order has been changed: {}", 
                    DisplayVec(&unknown_keys))
                );
            let unknown_key_set = unknown_keys.into_iter().collect::<HashSet<_>>();
            let update_ord_data = self.update_order_entries(None, &unknown_key_set);
            self.write_to_file().map_err(UnknownKeyErr::empty_with_err)?;
            return Err(UnknownKeyErr {
                err,
                unknown_keys: Some(unknown_key_set),
                update_ord_data: Some(update_ord_data),
            });
        }
        Err(UnknownKeyErr {
            err: std::io::Error::other(format!(
                "Found load order set for the following file(s) not registered with the app: {}",
                DisplayVec(&unknown_keys)
            )),
            unknown_keys: Some(unknown_keys.into_iter().collect::<HashSet<_>>()),
            update_ord_data: None,
        })
    }

    /// returns an owned `HashMap` with values parsed into K: `String`, V: `usize`  
    /// this function also fixes usize.parse() errors and if values are out of order
    #[instrument(level = "trace", skip_all)]
    pub fn parse_section(&mut self, unknown_keys: &HashSet<String>) -> std::io::Result<OrderMap> {
        let mut write_to_file = false;
        if self.section().contains_key(LOADER_EXAMPLE) {
            self.mut_section().remove(LOADER_EXAMPLE);
            write_to_file = true;
            info!("Removed: '{LOADER_EXAMPLE}' from: {}", LOADER_FILES[3]);
        }
        if self.mods_is_empty() {
            trace!("No mods have load order");
            if write_to_file {
                self.write_to_file()?
            }
            return Ok(HashMap::new());
        }
        let map = self.parse_into_map();
        if self.mods_registered() != map.len() {
            trace!("fixing usize parse error in: {}", LOADER_FILES[3]);
            self.update_order_entries(None, unknown_keys);
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
                self.update_order_entries(None, unknown_keys);
                self.write_to_file()?;
                info!(
                    "Found entries out of order, sorted load order entries in: {}",
                    LOADER_FILES[3]
                );
                return Ok(self.parse_into_map());
            }
        }
        if write_to_file {
            self.write_to_file()?
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

    /// updates the load order values in `Some("loadorder")` so there are no gaps in values  
    /// if you want a key's value to remain the unedited you can supply `Some(stable_key)`  
    /// this also calculates the correct max_order val (same logic appears in `[RegMod].max_order()`)  
    /// && stores any missing values in range `1..high_order`
    ///
    /// **NOTE:** this fn does not write any updated changes to file
    #[instrument(level = "trace", skip(self))]
    pub fn update_order_entries(
        &mut self,
        stable: Option<&str>,
        unknown_keys: &HashSet<String>,
    ) -> OrdMetaData {
        if self.mods_is_empty() {
            trace!("nothing to update");
            return OrdMetaData {
                max_order: (0, false),
                missing_vals: None,
            };
        }
        let mut k_v = Vec::with_capacity(self.section().len());
        let mut input_vals = HashSet::with_capacity(self.section().len());
        let (mut stable_k, mut stable_v) = ("", 69420_usize);
        for (k, v) in self.iter() {
            if k == LOADER_EXAMPLE {
                info!("Removed: '{LOADER_EXAMPLE}' from: {}", LOADER_FILES[3]);
                continue;
            }
            let curr_v = v.parse::<usize>().unwrap_or_else(|_| {
                if unknown_keys.contains(k) {
                    usize::MAX
                } else {
                    usize::MAX / 2
                }
            });
            input_vals.insert(curr_v);
            if let Some(input_k) = stable {
                if k == input_k {
                    (stable_k, stable_v) = (k, curr_v);
                    continue;
                }
            }
            k_v.push((k, curr_v));
        }
        k_v.sort_by_key(|(_, v)| *v);

        let mut missing_vals = Vec::new();
        let mut new_section = ini::Properties::new();
        let (max_order, missing_vals) = if k_v.is_empty() && !stable_k.is_empty() {
            new_section.append(
                stable_k,
                if stable_v == 0 {
                    "0"
                } else if stable_v == 1 {
                    "1"
                } else {
                    missing_vals.push(1);
                    "1"
                },
            );
            ((1, false), Some(missing_vals).filter(|v| !v.is_empty()))
        } else {
            let mut offset: usize = if (!k_v.is_empty() && k_v[0].1 == 0) || stable_v == 0 {
                0
            } else {
                1
            };
            let mut last_user_val = 0_usize;
            let mut check_for_missing_val = |offset: &usize| {
                if *offset > 0 && input_vals.insert(*offset) {
                    missing_vals.push(*offset);
                }
            };
            let mut iter = k_v.iter().peekable();
            while let Some((k, v)) = iter.next() {
                check_for_missing_val(&offset);
                if !stable_k.is_empty() && (stable_v == offset || stable_v == *v) {
                    last_user_val = offset;
                    new_section.append(std::mem::take(&mut stable_k), offset.to_string());
                    if *v != stable_v && *v > offset {
                        offset += 1;
                    }
                    check_for_missing_val(&offset);
                }
                if !unknown_keys.contains(*k) {
                    last_user_val = offset;
                } else if offset == last_user_val {
                    offset += 1;
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
            if !stable_k.is_empty() {
                last_user_val = offset;
                new_section.append(stable_k, &offset.to_string());
            }
            let last_key = new_section.iter().nth(new_section.len() - 1).map(|(k, _)| k).unwrap();
            let end_user_offset = last_user_val.to_string();
            (
                if new_section.iter().filter(|(_, v)| *v == end_user_offset).count() <= 1 {
                    (last_user_val, false)
                } else {
                    (last_user_val + 1, true)
                },
                if !missing_vals.is_empty() {
                    if *missing_vals.last().unwrap() == offset && unknown_keys.contains(last_key) {
                        missing_vals.pop();
                    }
                    Some(missing_vals).filter(|v| !v.is_empty())
                } else {
                    None
                },
            )
        };
        std::mem::swap(self.mut_section(), &mut new_section);
        trace!("re-calculated the order of entries in {}", LOADER_FILES[3]);
        OrdMetaData {
            max_order,
            missing_vals,
        }
    }
}

pub trait RegModsExt {
    /// returns the calculation for the correct (`max_order`, `high_val.count() > 1`)
    fn max_order(&self) -> (usize, bool);
}

impl RegModsExt for [RegMod] {
    fn max_order(&self) -> (usize, bool) {
        let set_indices = self
            .iter()
            .enumerate()
            .filter(|(_, m)| m.order.set)
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        let len = set_indices.len();
        if len < 2 {
            return (len, false);
        }
        let high_order = set_indices
            .iter()
            .map(|&i| self[i].order.at)
            .max()
            .expect("order set to a usize");
        if set_indices
            .iter()
            .filter(|&&i| self[i].order.at == high_order)
            .count()
            == 1
        {
            (high_order, false)
        } else {
            (high_order + 1, true)
        }
    }
}

use log::{trace, warn};
use std::{
    collections::{HashMap, HashSet},
    io::ErrorKind,
    path::{Path, PathBuf},
};

use crate::{
    does_dir_contain, new_io_error, utils::ini::{
        parser::RegMod,
        writer::new_cfg,
        common::{ModLoaderCfg, WriteToFile},
    }, Operation, OperationResult, LOADER_FILES, FileData
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

impl ModLoaderCfg {
    pub fn verify_keys(&mut self, mods: &[RegMod]) -> std::io::Result<()> {
        let valid_dlls = mods
            .iter()
            .flat_map(|m| {
                m.files
                    .dll
                    .iter()
                    .filter_map(|f| {
                        Some({
                            let file_name = f.file_name()?.to_string_lossy();
                            let file_data = FileData::from(&file_name);
                            format!("{}{}", file_data.name, file_data.extension)
                        })
                    })
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

    /// updates the load order values in `Some("loadorder")` so they are always `0..`  
    /// if you want a key's value to remain the unedited you can supply `Some(stable_key)`  
    /// then writes the updated key values to file
    ///
    /// error cases:  
    ///     section is not set to "loadorder"  
    ///     fails to write to file  
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
        self.write_to_file()
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
use ini::Ini;
use log::trace;
use std::{
    collections::HashMap,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use crate::{
    does_dir_contain, get_cfg, new_io_error,
    utils::ini::{
        parser::{IniProperty, ModError, RegMod, Setup},
        writer::{new_cfg, EXT_OPTIONS},
    },
    Operation, OperationResult, LOADER_FILES, LOADER_KEYS, LOADER_SECTIONS,
};

#[derive(Default)]
pub struct ModLoader {
    installed: bool,
    disabled: bool,
    path: PathBuf,
}

impl ModLoader {
    pub fn properties(game_dir: &Path) -> std::io::Result<ModLoader> {
        let cfg_dir = game_dir.join(LOADER_FILES[2]);
        match does_dir_contain(game_dir, Operation::Count, &LOADER_FILES) {
            Ok(OperationResult::Count((_, files))) => {
                if files.contains(LOADER_FILES[1]) || !files.contains(LOADER_FILES[0]) {
                    if !files.contains(LOADER_FILES[2]) {
                        new_cfg(&cfg_dir)?;
                    }
                    Ok(ModLoader {
                        installed: true,
                        disabled: false,
                        path: cfg_dir,
                    })
                } else if files.contains(LOADER_FILES[0]) || !files.contains(LOADER_FILES[1]) {
                    if !files.contains(LOADER_FILES[2]) {
                        new_cfg(&cfg_dir)?;
                    }
                    Ok(ModLoader {
                        installed: true,
                        disabled: true,
                        path: cfg_dir,
                    })
                } else {
                    return new_io_error!(
                        ErrorKind::InvalidData,
                        format!(
                            "Elden Mod Loader is not installed at: {}",
                            game_dir.display()
                        )
                    );
                }
            }
            Err(err) => Err(err),
            _ => unreachable!(),
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

#[derive(Default)]
pub struct ModLoaderCfg {
    cfg: Ini,
    cfg_dir: PathBuf,
    section: Option<String>,
}

impl ModLoaderCfg {
    pub fn read_section(game_dir: &Path, section: Option<&str>) -> std::io::Result<ModLoaderCfg> {
        if section.is_none() {
            return new_io_error!(ErrorKind::InvalidInput, "section can not be none");
        }
        let cfg_dir = ModLoader::properties(game_dir)?.path;
        let mut cfg = get_cfg(&cfg_dir)?;
        if !cfg.is_setup(&LOADER_SECTIONS) {
            new_cfg(&cfg_dir)?;
        }
        if cfg.section(section).is_none() {
            cfg.init_section(section)?
        }
        Ok(ModLoaderCfg {
            cfg,
            cfg_dir,
            section: section.map(String::from),
        })
    }

    pub fn get_load_delay(&self) -> std::io::Result<u32> {
        match IniProperty::<u32>::read(&self.cfg, LOADER_SECTIONS[0], LOADER_KEYS[0], false) {
            Ok(delay_time) => Ok(delay_time.value),
            Err(err) => Err(err.add_msg(format!(
                "Found an unexpected character saved in \"{}\"",
                LOADER_KEYS[0]
            ))),
        }
    }

    pub fn get_show_terminal(&self) -> std::io::Result<bool> {
        match IniProperty::<bool>::read(&self.cfg, LOADER_SECTIONS[0], LOADER_KEYS[1], false) {
            Ok(delay_time) => Ok(delay_time.value),
            Err(err) => Err(err.add_msg(format!(
                "Found an unexpected character saved in \"{}\"",
                LOADER_KEYS[1]
            ))),
        }
    }

    #[inline]
    pub fn mut_section(&mut self) -> &mut ini::Properties {
        self.cfg.section_mut(self.section.as_ref()).unwrap()
    }

    #[inline]
    fn section(&self) -> &ini::Properties {
        self.cfg.section(self.section.as_ref()).unwrap()
    }

    #[inline]
    fn iter(&self) -> ini::PropertyIter {
        self.section().iter()
    }

    /// Returns an owned `HashMap` with values parsed into K: `String`, V: `usize`  
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

    fn parse_into_map(&self) -> HashMap<String, usize> {
        self.iter()
            .filter_map(|(k, v)| Some((k.to_string(), v.parse::<usize>().ok()?)))
            .collect::<HashMap<String, usize>>()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.section().is_empty()
    }

    #[inline]
    pub fn path(&self) -> &Path {
        &self.cfg_dir
    }

    pub fn write_to_file(&self) -> std::io::Result<()> {
        self.cfg.write_to_file_opt(&self.cfg_dir, EXT_OPTIONS)
    }

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

impl<'a> Countable for &'a [RegMod] {
    #[inline]
    fn order_count(&self) -> usize {
        self.iter().filter(|m| m.order.set).count()
    }
}

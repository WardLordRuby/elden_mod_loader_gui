use ini::Ini;
use log::{error, info, trace};
use std::path::{Path, PathBuf};

use crate::{
    utils::ini::{
        parser::{IniProperty, RegMod},
        writer::EXT_OPTIONS,
    },
    LOADER_KEYS, LOADER_SECTIONS,
    {does_dir_contain, get_cfg, Operation, LOADER_FILES, LOADER_FILES_DISABLED},
};

#[derive(Default)]
pub struct ModLoader {
    installed: bool,
    disabled: bool,
    path: PathBuf,
}

impl ModLoader {
    pub fn properties(game_dir: &Path) -> ModLoader {
        match does_dir_contain(game_dir, Operation::All, &LOADER_FILES) {
            Ok(true) => {
                info!("Found mod loader files");
                ModLoader {
                    installed: true,
                    disabled: false,
                    path: game_dir.join(LOADER_FILES[0]),
                }
            }
            Ok(false) => {
                trace!("Checking if mod loader is disabled");
                match does_dir_contain(game_dir, Operation::All, &LOADER_FILES_DISABLED) {
                    Ok(true) => {
                        info!("Found mod loader files in the disabled state");
                        ModLoader {
                            installed: true,
                            disabled: true,
                            path: game_dir.join(LOADER_FILES[0]),
                        }
                    }
                    Ok(false) => {
                        error!("Mod Loader Files not found in selected path");
                        ModLoader::default()
                    }
                    Err(err) => {
                        error!("{err}");
                        ModLoader::default()
                    }
                }
            }
            Err(err) => {
                error!("{err}");
                ModLoader::default()
            }
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
}

#[derive(Default)]
pub struct ModLoaderCfg {
    cfg: Ini,
    cfg_dir: PathBuf,
    section: Option<String>,
}

impl ModLoaderCfg {
    pub fn read_section(game_dir: &Path, section: Option<&str>) -> Result<ModLoaderCfg, String> {
        if section.is_none() {
            return Err(String::from("section can not be none"));
        }
        let cfg_dir = match does_dir_contain(game_dir, Operation::All, &[LOADER_FILES[0]]) {
            Ok(true) => game_dir.join(LOADER_FILES[0]),
            Ok(false) => {
                return Err(String::from(
                    "\"mod_loader_config.ini\" does not exist in the current game_dir",
                ))
            }
            Err(err) => return Err(err.to_string()),
        };
        let mut cfg = match get_cfg(&cfg_dir) {
            Ok(ini) => ini,
            Err(err) => return Err(format!("Could not read \"mod_loader_config.ini\"\n{err}")),
        };
        if cfg.section(section).is_none() {
            ModLoaderCfg::init_section(&mut cfg, section)?
        }
        Ok(ModLoaderCfg {
            cfg,
            cfg_dir,
            section: section.map(String::from),
        })
    }

    pub fn update_section(&mut self, section: Option<&str>) -> Result<(), String> {
        if self.cfg.section(section).is_none() {
            ModLoaderCfg::init_section(&mut self.cfg, section)?
        };
        Ok(())
    }

    fn init_section(cfg: &mut ini::Ini, section: Option<&str>) -> Result<(), String> {
        trace!(
            "Section: \"{}\" not found creating new",
            section.expect("Passed in section not valid")
        );
        cfg.with_section(section).set("setter_temp_val", "0");
        if cfg.delete_from(section, "setter_temp_val").is_none() {
            return Err(format!(
                "Failed to create a new section: \"{}\"",
                section.unwrap()
            ));
        };
        Ok(())
    }

    pub fn get_load_delay(&self) -> Result<u32, String> {
        match IniProperty::<u32>::read(&self.cfg, LOADER_SECTIONS[0], LOADER_KEYS[0], false) {
            Some(delay_time) => Ok(delay_time.value),
            None => Err(format!(
                "Found an unexpected character saved in \"{}\"",
                LOADER_KEYS[0]
            )),
        }
    }

    pub fn get_show_terminal(&self) -> Result<bool, String> {
        match IniProperty::<bool>::read(&self.cfg, LOADER_SECTIONS[0], LOADER_KEYS[1], false) {
            Some(delay_time) => Ok(delay_time.value),
            None => Err(format!(
                "Found an unexpected character saved in \"{}\"",
                LOADER_KEYS[0]
            )),
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

    /// Returns an owned `Vec` with values parsed into `usize`
    pub fn parse_section(&self) -> Result<Vec<(String, usize)>, std::num::ParseIntError> {
        self.iter()
            .map(|(k, v)| {
                let parse_v = v.parse::<usize>();
                Ok((k.to_string(), parse_v?))
            })
            .collect::<Result<Vec<(String, usize)>, _>>()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.section().is_empty()
    }

    #[inline]
    pub fn dir(&self) -> &Path {
        &self.cfg_dir
    }

    pub fn write_to_file(&self) -> std::io::Result<()> {
        self.cfg.write_to_file_opt(&self.cfg_dir, EXT_OPTIONS)
    }
}

pub fn update_order_entries(
    stable: Option<&str>,
    section: &mut ini::Properties,
) -> Result<(), std::num::ParseIntError> {
    let mut k_v = Vec::with_capacity(section.len());
    let (mut stable_k, mut stable_v) = (String::new(), 0_usize);
    for (k, v) in section.clone() {
        section.remove(&k);
        if let Some(new_k) = stable {
            if k == new_k {
                (stable_k, stable_v) = (k, v.parse::<usize>()?);
                continue;
            }
        }
        k_v.push((k, v.parse::<usize>()?));
    }
    k_v.sort_by_key(|(_, v)| *v);
    if k_v.is_empty() && !stable_k.is_empty() {
        section.append(&stable_k, "0");
    } else {
        let mut offset = 0_usize;
        for (k, _) in k_v {
            if !stable_k.is_empty() && !section.contains_key(&stable_k) && stable_v == offset {
                section.append(&stable_k, stable_v.to_string());
                offset += 1;
            }
            section.append(k, offset.to_string());
            offset += 1;
        }
        if !stable_k.is_empty() && !section.contains_key(&stable_k) {
            section.append(&stable_k, offset.to_string())
        }
    }
    Ok(())
}

pub trait Countable {
    fn order_count(&self) -> usize;
}

impl<'a> Countable for &'a [RegMod] {
    fn order_count(&self) -> usize {
        self.iter().filter(|m| m.order.set).count()
    }
}

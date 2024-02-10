use ini::Ini;
use log::{debug, error, info, warn};
use std::{
    path::{Path, PathBuf},
    str::FromStr,
    sync::{mpsc, Arc},
    thread,
};

use crate::{
    get_cfg,
    ini_tools::writer::{remove_array, remove_entry},
};

pub struct IniProperty<T: ValueType> {
    //section: Option<String>,
    //key: String,
    pub value: T,
}

pub trait ValueType: Sized {
    fn parse_str(ini: &Ini, section: Option<&str>, key: &str) -> Option<Self>;
}

impl ValueType for bool {
    fn parse_str(ini: &Ini, section: Option<&str>, key: &str) -> Option<Self> {
        let ini_value = ini.get_from(section, key).unwrap();
        match bool::from_str(ini_value.to_lowercase().as_str()) {
            Ok(_) => Some(bool::from_str(ini_value).unwrap()),
            Err(err) => {
                error!("Error: {}", err);
                None
            }
        }
    }
}

impl ValueType for PathBuf {
    fn parse_str(ini: &Ini, section: Option<&str>, key: &str) -> Option<Self> {
        let ini_path = PathBuf::from(ini.get_from(section, key).unwrap());
        if section == Some("mod-files") {
            let game_dir = IniProperty::<PathBuf>::read(ini, Some("paths"), "game_dir")
                .unwrap()
                .value;
            if validate_path(&game_dir.join(&ini_path)) {
                Some(ini_path)
            } else {
                None
            }
        } else if validate_path(&ini_path) {
            Some(ini_path)
        } else {
            None
        }
    }
}

impl ValueType for Vec<PathBuf> {
    fn parse_str(ini: &Ini, section: Option<&str>, key: &str) -> Option<Self> {
        let ini_section = ini.section(section).unwrap();
        if ini_section.get(key).unwrap() != "array" {
            panic!(
                "Parse Vec<PathBuf>: array expected got {}",
                ini_section.get(key).unwrap()
            );
        }

        let ini_files: Vec<PathBuf> = ini_section
            .iter()
            .skip_while(|(k, _)| *k != key)
            .skip_while(|(k, _)| *k == key)
            .take_while(|(k, _)| *k == "array[]")
            .map(|(_, v)| PathBuf::from(v))
            .collect();

        let game_dir = IniProperty::<PathBuf>::read(ini, Some("paths"), "game_dir")
            .unwrap()
            .value;
        if ini_files
            .iter()
            .all(|path| validate_path(&game_dir.join(path)))
        {
            Some(ini_files)
        } else {
            None
        }
    }
}

impl<T: ValueType> IniProperty<T> {
    pub fn read(ini: &Ini, section: Option<&str>, key: &str) -> Result<IniProperty<T>, String> {
        let format_key = key.replace(' ', "_");
        match IniProperty::is_valid(ini, section, &format_key) {
            Some(value) => {
                debug!(
                    "Success: read key: \"{}\" Section: \"{}\" from ini",
                    key,
                    section.unwrap()
                );
                Ok(IniProperty {
                    //section: Some(section.unwrap().to_string()),
                    //key: key.to_string(),
                    value,
                })
            }
            None => Err(format!(
                "Value stored in Section: \"{}\", Key: \"{}\" is not valid",
                section.unwrap(),
                key
            )),
        }
    }

    fn is_valid(ini: &Ini, section: Option<&str>, key: &str) -> Option<T> {
        match &ini.section(section) {
            Some(s) => match s.contains_key(key) {
                true => T::parse_str(ini, section, key),
                false => {
                    warn!("Key: \"{}\" not found in {:?}", key, ini);
                    None
                }
            },
            None => {
                warn!("Section: \"{}\" not found in {:?}", section.unwrap(), ini);
                None
            }
        }
    }
}

fn validate_path(path: &Path) -> bool {
    match path.try_exists() {
        Ok(result) => {
            if result {
                true
            } else {
                warn!("Path from ini can not be found on machine");
                false
            }
        }
        Err(err) => {
            error!("Error: {}", err);
            false
        }
    }
}

pub struct RegMod {
    pub name: String,
    pub state: bool,
    pub files: Vec<PathBuf>,
}

impl RegMod {
    pub fn collect(path: &str) -> Vec<Self> {
        fn sync_keys(path: &str) {
            let ini = get_cfg(path).unwrap();
            let mod_state_data = ini.section(Some("registered-mods")).unwrap().clone();
            let mod_files_data = ini.section(Some("mod-files")).unwrap().clone();
            let state_keys: Vec<&str> = mod_state_data.iter().map(|(k, _)| k).collect();
            let file_keys: Vec<&str> = mod_files_data
                .iter()
                .filter_map(|(k, _)| if k != "array[]" { Some(k) } else { None })
                .collect();
            for key in state_keys.iter().filter(|&&k| !file_keys.contains(&k)) {
                warn!("\"{}\" has no matching files", &key);
                remove_entry(path, Some("registered-mods"), key);
            }
            for key in file_keys.iter().filter(|&&k| !state_keys.contains(&k)) {
                if mod_files_data.get(key).unwrap() == "array" {
                    warn!("\"{}\" has no matching state", &key);
                    remove_array(path, key);
                } else {
                    warn!("\"{}\" has no matching state", &key);
                    remove_entry(path, Some("mod-files"), key);
                }
            }
        }
        sync_keys(path);
        let ini = Arc::new(get_cfg(path).unwrap());
        let mod_state_data = ini.section(Some("registered-mods")).unwrap();
        let mut found_data: Vec<RegMod> = Vec::with_capacity(mod_state_data.len());
        let (tx, rx) = mpsc::channel();
        for (key, _) in mod_state_data.iter() {
            let ini_clone = Arc::clone(&ini);
            let key_clone = String::from(key);
            let tx_clone = tx.clone();
            thread::spawn(move || {
                let name = key_clone.replace('_', " ");
                let state =
                    IniProperty::<bool>::read(&ini_clone, Some("registered-mods"), &key_clone)
                        .unwrap()
                        .value;

                let files = if ini_clone.get_from(Some("mod-files"), &key_clone).unwrap() == "array"
                {
                    IniProperty::<Vec<PathBuf>>::read(&ini_clone, Some("mod-files"), &key_clone) // <-- Check if a closing parenthesis is needed here
                        .unwrap()
                        .value
                } else {
                    vec![
                        IniProperty::<PathBuf>::read(&ini_clone, Some("mod-files"), &key_clone)
                            .unwrap()
                            .value,
                    ]
                };
                tx_clone.send(RegMod { name, state, files }).unwrap();
            });
        }
        drop(tx);
        for received in rx {
            found_data.push(received);
        }
        found_data
    }
}

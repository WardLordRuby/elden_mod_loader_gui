use ini::Ini;
use log::{error, info, warn};

use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

pub struct IniProperty<T: ValueType> {
    pub section: Option<String>,
    pub key: String,
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
        if validate_path(&ini_path) {
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
        match IniProperty::is_valid(ini, section, key) {
            Some(value) => {
                info!("Success: read \"{}\" from ini", key);
                Ok(IniProperty {
                    section: Some(section.unwrap().to_string()),
                    key: key.to_string(),
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

    //fn remove_array
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

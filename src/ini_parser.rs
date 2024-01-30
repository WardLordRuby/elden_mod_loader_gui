use ini::Ini;
use log::{error, info, warn};
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub struct IniProperty<T: ValueType> {
    pub section: Option<String>,
    pub key: String,
    pub value: T,
}

pub trait ValueType: Sized {
    fn parse_str(value: &str) -> Option<Self>;
}

impl ValueType for bool {
    fn parse_str(ini_value: &str) -> Option<Self> {
        match bool::from_str(ini_value) {
            Ok(_) => Some(bool::from_str(ini_value).unwrap()),
            Err(err) => {
                error!("Error: {}", err);
                None
            }
        }
    }
}

impl ValueType for PathBuf {
    fn parse_str(ini_value: &str) -> Option<Self> {
        match Path::new(ini_value).try_exists() {
            Ok(result) => {
                if result {
                    Some(PathBuf::from(ini_value))
                } else {
                    warn!("Path from ini can not be found on machine");
                    None
                }
            }
            Err(err) => {
                error!("Error: {}", err);
                None
            }
        }
    }
}

impl<T: ValueType> IniProperty<T> {
    pub fn new(ini: &Ini, section: Option<&str>, key: &str) -> Result<IniProperty<T>, String> {
        match IniProperty::is_valid(ini, section, key) {
            Some(value) => {
                info!("Sucessfuly read \"{}\" from ini", key);
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
                true => T::parse_str(ini.get_from(section, key).unwrap()),
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

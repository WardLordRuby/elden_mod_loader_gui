use ini::{Ini, Properties};
use log::{debug, error, info, warn};
use std::{
    collections::HashMap,
    fmt::Debug,
    io,
    path::{Path, PathBuf},
    str::ParseBoolError,
};

use crate::{
    get_cfg,
    ini_tools::writer::{remove_array, remove_entry},
};

pub trait ValueType: Sized {
    type ParseError: Debug;
    fn parse_str(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        skip_validation: bool,
    ) -> Result<Self, Self::ParseError>;
    fn validate(
        self,
        ini: &Ini,
        section: Option<&str>,
        disable: bool,
    ) -> Result<Self, Self::ParseError>;
}

impl ValueType for bool {
    type ParseError = ParseBoolError;
    fn parse_str(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        _skip_validation: bool,
    ) -> Result<Self, Self::ParseError> {
        ini.get_from(section, key).unwrap().parse::<bool>()
    }
    // Do not use | no extra steps needed for validating a bool, .parse already handles validation or ParseBoolError
    fn validate(
        self,
        _ini: &Ini,
        _section: Option<&str>,
        _disable: bool,
    ) -> Result<Self, Self::ParseError> {
        Ok(self)
    }
}

impl ValueType for PathBuf {
    type ParseError = io::Error;
    fn parse_str(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        skip_validation: bool,
    ) -> Result<Self, Self::ParseError> {
        if skip_validation {
            Ok(PathBuf::from(ini.get_from(section, key).unwrap()))
        } else {
            PathBuf::from(ini.get_from(section, key).unwrap()).validate(
                ini,
                section,
                skip_validation,
            )
        }
    }
    fn validate(
        self,
        ini: &Ini,
        section: Option<&str>,
        disable: bool,
    ) -> Result<Self, Self::ParseError> {
        if !disable {
            if section == Some("mod-files") {
                let game_dir = IniProperty::<PathBuf>::read(ini, Some("paths"), "game_dir", false)
                    .unwrap()
                    .value;
                match validate_path(&game_dir.join(&self)) {
                    Ok(()) => Ok(self),
                    Err(err) => Err(err),
                }
            } else {
                match validate_path(&self) {
                    Ok(()) => Ok(self),
                    Err(err) => Err(err),
                }
            }
        } else {
            Ok(self)
        }
    }
}

impl ValueType for Vec<PathBuf> {
    type ParseError = io::Error;
    fn parse_str(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        skip_validation: bool,
    ) -> Result<Self, Self::ParseError> {
        fn read_array(section: &Properties, key: &str) -> Vec<PathBuf> {
            section
                .iter()
                .skip_while(|(k, _)| *k != key)
                .skip_while(|(k, _)| *k == key)
                .take_while(|(k, _)| *k == "array[]")
                .map(|(_, v)| PathBuf::from(v))
                .collect()
        }
        if skip_validation {
            Ok(read_array(ini.section(section).unwrap(), key))
        } else {
            read_array(ini.section(section).unwrap(), key).validate(ini, section, skip_validation)
        }
    }
    fn validate(
        self,
        ini: &Ini,
        _section: Option<&str>,
        disable: bool,
    ) -> Result<Self, Self::ParseError> {
        if !disable {
            let game_dir = IniProperty::<PathBuf>::read(ini, Some("paths"), "game_dir", false)
                .unwrap()
                .value;
            if let Some(err) = self
                .iter()
                .find_map(|path| validate_path(&game_dir.join(path)).err())
            {
                Err(err)
            } else {
                Ok(self)
            }
        } else {
            Ok(self)
        }
    }
}

fn validate_path(path: &Path) -> Result<(), io::Error> {
    match path.try_exists() {
        Ok(result) => {
            if result {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Path: \"{}\" can not be found on machine", path.display()),
                ))
            }
        }
        Err(_) => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "Path \"{}\"'s existance can neither be confirmed nor denied",
                path.display()
            ),
        )),
    }
}

pub struct IniProperty<T: ValueType> {
    //section: Option<String>,
    //key: String,
    pub value: T,
}

impl<T: ValueType> IniProperty<T> {
    pub fn read(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        skip_validation: bool,
    ) -> Option<IniProperty<T>> {
        let format_key = key.replace(' ', "_");
        match IniProperty::is_valid(ini, section, &format_key, skip_validation) {
            Ok(value) => {
                debug!(
                    "Success: read key: \"{}\" Section: \"{}\" from ini",
                    key,
                    section.unwrap()
                );
                Some(IniProperty {
                    //section: Some(section.unwrap().to_string()),
                    //key: key.to_string(),
                    value,
                })
            }
            Err(err) => {
                error!(
                    "{}",
                    format!(
                        "Value stored in Section: \"{}\", Key: \"{}\" is not valid",
                        section.unwrap(),
                        key
                    )
                );
                error!("Error: {}", err);
                None
            }
        }
    }

    fn is_valid(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        skip_validation: bool,
    ) -> Result<T, String> {
        match &ini.section(section) {
            Some(s) => match s.contains_key(key) {
                true => match T::parse_str(ini, section, key, skip_validation) {
                    Ok(t) => Ok(t),
                    Err(err) => Err(format!("Error: {:?}", err)),
                },
                false => Err(format!("Key: \"{}\" not found in {:?}", key, ini)),
            },
            None => Err(format!(
                "Section: \"{}\" not found in {:?}",
                section.unwrap(),
                ini
            )),
        }
    }
}

pub struct RegMod {
    pub name: String,
    pub state: bool,
    pub files: Vec<PathBuf>,
}

impl RegMod {
    pub fn collect(path: &str, skip_validation: bool) -> Vec<Self> {
        fn sync_keys<'a>(
            ini: &'a Ini,
            path: &str,
        ) -> HashMap<&'a str, (Result<bool, ParseBoolError>, Vec<PathBuf>)> {
            fn collect_file_data(section: &Properties) -> HashMap<&str, Vec<&str>> {
                section
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| v.0 != "array[]")
                    .map(|(i, v)| {
                        let paths = section
                            .iter()
                            .skip(i + 1)
                            .take_while(|v| v.0 == "array[]")
                            .map(|v| v.1)
                            .collect();
                        (v.0, if v.1 == "array" { paths } else { vec![v.1] })
                    })
                    .collect()
            }
            fn combine_map_data<'a>(
                state_map: HashMap<&'a str, &str>,
                file_map: HashMap<&str, Vec<&str>>,
            ) -> HashMap<&'a str, (Result<bool, ParseBoolError>, Vec<PathBuf>)> {
                state_map
                    .iter()
                    .filter_map(|(&key, &value1)| {
                        file_map.get(&key).map(|value2| {
                            (
                                key,
                                (
                                    value1.parse::<bool>(),
                                    value2.iter().map(PathBuf::from).collect::<Vec<_>>(),
                                ),
                            )
                        })
                    })
                    .collect()
            }
            let mod_state_data = ini.section(Some("registered-mods")).unwrap();
            let mod_files_data = ini.section(Some("mod-files")).unwrap();
            let mut state_data = mod_state_data.iter().collect::<HashMap<&str, &str>>();
            let mut file_data = collect_file_data(mod_files_data);
            let invalid_state: Vec<_> = state_data
                .keys()
                .filter(|k| !file_data.contains_key(*k))
                .cloned()
                .collect();
            for key in invalid_state {
                state_data.remove(key);
                remove_entry(path, Some("registered-mods"), key);
                warn!("\"{}\" has no matching files", &key);
            }
            let invalid_files: Vec<_> = file_data
                .keys()
                .filter(|k| !state_data.contains_key(*k))
                .cloned()
                .collect();
            for key in invalid_files {
                if file_data.get(key).unwrap().len() > 1 {
                    remove_array(path, key);
                } else {
                    remove_entry(path, Some("mod-files"), key);
                }
                file_data.remove(key);
                warn!("\"{}\" has no matching state", &key);
            }
            combine_map_data(state_data, file_data)
        }
        fn collect_data_unsafe(ini: &Ini) -> Vec<(&str, &str, Vec<&str>)> {
            let mod_state_data = ini.section(Some("registered-mods")).unwrap();
            let mod_files_data = ini.section(Some("mod-files")).unwrap();
            mod_files_data
                .iter()
                .enumerate()
                .filter(|(_, v)| v.0 != "array[]")
                .map(|(i, v)| {
                    let paths: Vec<&str> = mod_files_data
                        .iter()
                        .skip(i + 1)
                        .take_while(|v| v.0 == "array[]")
                        .map(|v| v.1)
                        .collect();
                    let state = mod_state_data.get(v.0).unwrap();
                    (v.0, state, if v.1 == "array" { paths } else { vec![v.1] })
                })
                .collect()
        }
        let ini = get_cfg(path).unwrap();

        if skip_validation {
            let parsed_data = collect_data_unsafe(&ini);
            parsed_data
                .iter()
                .map(|v| RegMod {
                    name: v.0.replace('_', " ").to_string(),
                    state: v.1.parse::<bool>().unwrap(),
                    files: v.2.iter().map(PathBuf::from).collect(),
                })
                .collect()
        } else {
            let parsed_data = sync_keys(&ini, path);
            parsed_data
                .iter()
                .filter_map(|(k, v)| match &v.0 {
                    Ok(bool) => {
                        match v.1.len() {
                            1 => match v.1[0].to_owned().validate(
                                &ini,
                                Some("mod-files"),
                                skip_validation,
                            ) {
                                Ok(path) => Some(RegMod {
                                    name: k.replace('_', " ").to_string(),
                                    state: *bool,
                                    files: vec![path],
                                }),
                                Err(err) => {
                                    error!("Error: {}", err);
                                    None
                                }
                            },
                            2.. => match v.1.to_owned().validate(
                                &ini,
                                Some("mod-files"),
                                skip_validation,
                            ) {
                                Ok(paths) => Some(RegMod {
                                    name: k.replace('_', " ").to_string(),
                                    state: *bool,
                                    files: paths,
                                }),
                                Err(err) => {
                                    error!("Error: {}", err);
                                    None
                                }
                            },
                            0 => {
                                error!("Error: Tried to validate a Path in a Vec with size 0");
                                None
                            }
                        }
                    }
                    Err(err) => {
                        error!("Error: {}", err);
                        None
                    }
                })
                .collect()
        }
    }
}
// ----------------------Optimized original implementation-------------------------------
// let mod_state_data = ini.section(Some("registered-mods")).unwrap();
// mod_state_data
//     .iter()
//     .map(|(key, _)| RegMod {
//         name: key.replace('_', " ").to_string(),
//         state: IniProperty::<bool>::read(&ini, Some("registered-mods"), key)
//             .unwrap()
//             .value,
//         files: if ini.get_from(Some("mod-files"), key).unwrap() == "array" {
//             IniProperty::<Vec<PathBuf>>::read(&ini, Some("mod-files"), key)
//                 .unwrap()
//                 .value
//         } else {
//             vec![
//                 IniProperty::<PathBuf>::read(&ini, Some("mod-files"), key)
//                     .unwrap()
//                     .value,
//             ]
//         },
//     })
//     .collect()
// ----------------------------------Multi-threaded attempt----------------------------------------
// SLOW ASF -- Prolly because of how parser is setup -- try setting up parse_str to only take a str as input
// then pass each thread the strings it needs to parse
// let ini = Arc::new(get_cfg(path).unwrap());
// let mod_state_data = ini.section(Some("registered-mods")).unwrap();
// let (tx, rx) = mpsc::channel();
// let mut found_data: Vec<RegMod> = Vec::with_capacity(mod_state_data.len());
// for (key, _) in mod_state_data.iter() {
//     let ini_clone = Arc::clone(&ini);
//     let key_clone = String::from(key);
//     let tx_clone = tx.clone();
//     thread::spawn(move || {
//         tx_clone
//             .send(RegMod {
//                 name: key_clone.replace('_', " "),
//                 state: IniProperty::<bool>::read(
//                     &ini_clone,
//                     Some("registered-mods"),
//                     &key_clone,
//                 )
//                 .unwrap()
//                 .value,
//                 files: if ini_clone.get_from(Some("mod-files"), &key_clone).unwrap()
//                     == "array"
//                 {
//                     IniProperty::<Vec<PathBuf>>::read(
//                         &ini_clone,
//                         Some("mod-files"),
//                         &key_clone,
//                     )
//                     .unwrap()
//                     .value
//                 } else {
//                     vec![
//                         IniProperty::<PathBuf>::read(
//                             &ini_clone,
//                             Some("mod-files"),
//                             &key_clone,
//                         )
//                         .unwrap()
//                         .value,
//                     ]
//                 },
//             })
//             .unwrap()
//     });
// }
// drop(tx);

// for received in rx {
//     found_data.push(received);
// }
// found_data

use ini::{Ini, Properties};
use log::{debug, error, info, warn};
use std::{
    collections::HashMap,
    convert::Infallible,
    path::{Path, PathBuf},
    str::ParseBoolError,
};

use crate::{
    get_cfg,
    ini_tools::writer::{remove_array, remove_entry},
};

pub struct IniProperty<'a, T: ValueType<'a>> {
    //section: Option<String>,
    //key: String,
    pub value: T,
    lifetime: std::marker::PhantomData<&'a ()>,
}

pub trait ValueType<'a>: Sized {
    type MyError;
    type MyType: 'a;
    fn retrieve(ini: &Ini, section: Option<&str>, key: &str, skip_validation: bool)
        -> Option<Self>;
    fn parse_str(input: Self::MyType) -> Result<Self, Self::MyError>;
    fn validate(
        input: Result<Self, Self::MyError>,
        ini: &Ini,
        section: Option<&str>,
        disable: bool,
    ) -> Option<Self>;
}

impl<'a> ValueType<'a> for bool {
    type MyError = ParseBoolError;
    type MyType = &'a str;
    fn retrieve(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        skip_validation: bool,
    ) -> Option<Self> {
        ValueType::validate(
            ValueType::parse_str(ini.get_from(section, key).unwrap()),
            ini,
            section,
            skip_validation,
        )
    }
    fn parse_str(input: Self::MyType) -> Result<Self, Self::MyError> {
        input.parse::<bool>()
    }
    fn validate(
        input: Result<Self, Self::MyError>,
        _ini: &Ini,
        _section: Option<&str>,
        _disable: bool,
    ) -> Option<Self> {
        match input {
            Ok(bool) => Some(bool),
            Err(err) => {
                error!("Error: {}", err);
                None
            }
        }
    }
}

impl<'a> ValueType<'a> for PathBuf {
    type MyError = Infallible;
    type MyType = &'a str;
    fn retrieve(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        skip_validation: bool,
    ) -> Option<Self> {
        ValueType::validate(
            ValueType::parse_str(ini.get_from(section, key).unwrap()),
            ini,
            section,
            skip_validation,
        )
    }
    fn parse_str(input: Self::MyType) -> Result<Self, Self::MyError> {
        input.parse::<PathBuf>()
    }
    fn validate(
        input: Result<Self, Self::MyError>,
        ini: &Ini,
        section: Option<&str>,
        disable: bool,
    ) -> Option<Self> {
        match input {
            Ok(path) => {
                if !disable {
                    if section == Some("mod-files") {
                        let game_dir =
                            IniProperty::<PathBuf>::read(ini, Some("paths"), "game_dir", false)
                                .unwrap()
                                .value;
                        if validate_path(&game_dir.join(&path)) {
                            Some(path)
                        } else {
                            None
                        }
                    } else if validate_path(&path) {
                        Some(path)
                    } else {
                        None
                    }
                } else {
                    Some(path)
                }
            }
            _ => None,
        }
    }
}

impl<'a> ValueType<'a> for Vec<PathBuf> {
    type MyError = Infallible;
    type MyType = Vec<&'a str>;
    fn retrieve(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        skip_validation: bool,
    ) -> Option<Self> {
        let array = read_array(ini.section(section).unwrap(), key);
        ValueType::validate(ValueType::parse_str(array), ini, section, skip_validation)
    }
    fn parse_str(input: Self::MyType) -> Result<Self, Self::MyError> {
        input.into_iter().map(|p| p.parse::<PathBuf>()).collect()
    }
    fn validate(
        input: Result<Self, Self::MyError>,
        ini: &Ini,
        _section: Option<&str>,
        disable: bool,
    ) -> Option<Self> {
        let mut game_dir = PathBuf::new();
        if !disable {
            game_dir = IniProperty::<PathBuf>::read(ini, Some("paths"), "game_dir", false)
                .unwrap()
                .value;
        }
        match input {
            Ok(paths) => {
                if !disable {
                    if paths.iter().all(|path| validate_path(&game_dir.join(path))) {
                        Some(paths)
                    } else {
                        None
                    }
                } else {
                    Some(paths)
                }
            }
            Err(err) => {
                error!("Error: {}", err);
                None
            }
        }
    }
}

impl<'a, T: ValueType<'a>> IniProperty<'a, T> {
    pub fn read(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        skip_validation: bool,
    ) -> Result<IniProperty<'a, T>, String> {
        let format_key = key.replace(' ', "_");
        match IniProperty::is_valid(ini, section, &format_key, skip_validation) {
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
                    lifetime: std::marker::PhantomData,
                })
            }
            None => Err(format!(
                "Value stored in Section: \"{}\", Key: \"{}\" is not valid",
                section.unwrap(),
                key
            )),
        }
    }

    pub fn validate(
        input: Result<T, T::MyError>,
        ini: &Ini,
        section: Option<&str>,
        disable: bool,
    ) -> Option<T>
    where
        T: ValueType<'a>,
    {
        T::validate(input, ini, section, disable)
    }

    fn is_valid(ini: &Ini, section: Option<&str>, key: &str, skip_validation: bool) -> Option<T> {
        match &ini.section(section) {
            Some(s) => match s.contains_key(key) {
                true => T::retrieve(ini, section, key, skip_validation),
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

fn read_array<'a>(section: &'a Properties, key: &str) -> Vec<&'a str> {
    section
        .iter()
        .skip_while(|(k, _)| *k != key)
        .skip_while(|(k, _)| *k == key)
        .take_while(|(k, _)| *k == "array[]")
        .map(|(_, v)| v)
        .collect()
}

#[derive(Debug)]
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
            fn collect_section(section: &Properties) -> HashMap<&str, Vec<&str>> {
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
            let mod_state_data = ini.section(Some("registered-mods")).unwrap();
            let mod_files_data = ini.section(Some("mod-files")).unwrap();
            let mut state_data = mod_state_data.iter().collect::<HashMap<&str, &str>>();
            let mut file_data = collect_section(mod_files_data);
            let invalid_state: Vec<_> = state_data
                .keys()
                .filter(|k| !file_data.contains_key(*k))
                .cloned()
                .collect();
            for key in invalid_state {
                state_data.remove(key);
                remove_entry(path, Some("registered-mods"), key);
                warn!("\"{}\" has no matching files", &key);
                eprintln!("\"{}\" has no matching files", &key);
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
                warn!("\"{}\" has no matching state", &key);
                eprintln!("\"{}\" has no matching state", &key);
                file_data.remove(key);
            }
            state_data
                .iter()
                .zip(file_data.iter())
                .map(|((&key, &value1), (_, value2))| {
                    (
                        key,
                        (
                            value1.parse::<bool>(),
                            value2.iter().map(PathBuf::from).collect(),
                        ),
                    )
                })
                .collect()
        }
        let ini = get_cfg(path).unwrap();
        let mut parsed_data = sync_keys(&ini, path);

        if skip_validation {
            parsed_data
                .into_iter()
                .map(|v| RegMod {
                    name: v.0.replace('_', " ").to_string(),
                    state: v.1 .0.unwrap(),
                    files: v.1 .1,
                })
                .collect()
        } else {
            parsed_data
                .drain()
                .filter_map(|(k, v)| {
                    let bool_validation =
                        IniProperty::<bool>::validate(v.0, &ini, Some("registered-mods"), false);
                    let paths_validation: Option<Vec<PathBuf>>;
                    if v.1.len() == 1 {
                        paths_validation = IniProperty::<PathBuf>::validate(
                            Ok(v.1),
                            &ini,
                            Some("mod-files"),
                            false,
                        );
                    } else {
                        paths_validation = IniProperty::<Vec<PathBuf>>::validate(
                            Ok(v.1),
                            &ini,
                            Some("mod-files"),
                            false,
                        );
                    }
                    bool_validation.and_then(|bool_value| {
                        paths_validation.map(|paths_value| RegMod {
                            name: k.replace('_', " ").to_string(),
                            state: bool_value,
                            files: paths_value,
                        })
                    })
                })
                .collect()
        }
        // data validation layer -- make a toggle so we can easily turn off for benchmarks but keep on for tests
    }
}
// ----------------------Optimized original implementation-------------------------------
// Collect into vecdeque tuple (key: &str, state: &str, files ?)
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

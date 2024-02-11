use ini::{Ini, Properties};
use log::{debug, error, info, warn};
use std::{
    collections::HashMap,
    convert::Infallible,
    marker::PhantomData,
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
    lifetime: PhantomData<&'a ()>,
}

pub trait ValueType<'a>: Sized {
    type MyError;
    type MyInput: 'a;
    fn retrieve(ini: &Ini, section: Option<&str>, key: &str, skip_validation: bool)
        -> Option<Self>;
    fn parse_str(input: Self::MyInput) -> Result<Self, Self::MyError>;
    fn validate(
        input: Result<Self, Self::MyError>,
        ini: &Ini,
        section: Option<&str>,
        disable: bool,
    ) -> Option<Self>;
}

impl<'a> ValueType<'a> for bool {
    type MyError = ParseBoolError;
    type MyInput = &'a str;
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
    fn parse_str(input: Self::MyInput) -> Result<Self, Self::MyError> {
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
    type MyInput = &'a str;
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
    fn parse_str(input: Self::MyInput) -> Result<Self, Self::MyError> {
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
    type MyInput = Vec<&'a str>;
    fn retrieve(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        skip_validation: bool,
    ) -> Option<Self> {
        fn read_array<'a>(section: &'a Properties, key: &str) -> Vec<&'a str> {
            section
                .iter()
                .skip_while(|(k, _)| *k != key)
                .skip_while(|(k, _)| *k == key)
                .take_while(|(k, _)| *k == "array[]")
                .map(|(_, v)| v)
                .collect()
        }
        let array = read_array(ini.section(section).unwrap(), key);
        ValueType::validate(ValueType::parse_str(array), ini, section, skip_validation)
    }
    fn parse_str(input: Self::MyInput) -> Result<Self, Self::MyError> {
        input.into_iter().map(|p| p.parse::<PathBuf>()).collect()
    }
    fn validate(
        input: Result<Self, Self::MyError>,
        ini: &Ini,
        _section: Option<&str>,
        disable: bool,
    ) -> Option<Self> {
        match input {
            Ok(paths) => {
                if !disable {
                    let game_dir =
                        IniProperty::<PathBuf>::read(ini, Some("paths"), "game_dir", false)
                            .unwrap()
                            .value;
                    if paths.iter().all(|path| validate_path(&game_dir.join(path))) {
                        Some(paths)
                    } else {
                        None
                    }
                } else {
                    Some(paths)
                }
            }
            _ => None,
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
                    lifetime: PhantomData,
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

pub struct RegMod {
    pub name: String,
    pub state: bool,
    pub files: Vec<PathBuf>,
}

impl RegMod {
    pub fn collect(path: &str, skip_validation: bool) -> Vec<Self> {
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
        fn collect_state_data(section: &Properties) -> HashMap<&str, &str> {
            section.iter().collect()
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
        fn sync_keys<'a>(
            ini: &'a Ini,
            path: &str,
        ) -> HashMap<&'a str, (Result<bool, ParseBoolError>, Vec<PathBuf>)> {
            let mod_state_data = ini.section(Some("registered-mods")).unwrap();
            let mod_files_data = ini.section(Some("mod-files")).unwrap();
            let mut state_data = collect_state_data(mod_state_data);
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
        let ini = get_cfg(path).unwrap();

        if skip_validation {
            let file_data = collect_file_data(ini.section(Some("mod-files")).unwrap());
            let state_data = collect_state_data(ini.section(Some("registered-mods")).unwrap());
            let parsed_data = combine_map_data(state_data, file_data);
            parsed_data
                .into_iter()
                .map(|v| RegMod {
                    name: v.0.replace('_', " ").to_string(),
                    state: v.1 .0.unwrap(),
                    files: v.1 .1,
                })
                .collect()
        } else {
            let parsed_data = sync_keys(&ini, path);
            parsed_data
                .into_iter()
                .filter_map(|(k, v)| {
                    if let Some(bool) = IniProperty::<bool>::validate(
                        v.0,
                        &ini,
                        Some("registered-mods"),
                        skip_validation,
                    ) {
                        match v.1.len() {
                            1 => IniProperty::<PathBuf>::validate(
                                Ok(v.1[0].clone()),
                                &ini,
                                Some("mod-files"),
                                skip_validation,
                            )
                            .map(|path| RegMod {
                                name: k.replace('_', " ").to_string(),
                                state: bool,
                                files: vec![path],
                            }),
                            2.. => IniProperty::<Vec<PathBuf>>::validate(
                                Ok(v.1),
                                &ini,
                                Some("mod-files"),
                                skip_validation,
                            )
                            .map(|paths| RegMod {
                                name: k.replace('_', " ").to_string(),
                                state: bool,
                                files: paths,
                            }),
                            0 => {
                                error!("Error: Tried to validate a Path in a Vec with size 0");
                                None
                            }
                        }
                    } else {
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

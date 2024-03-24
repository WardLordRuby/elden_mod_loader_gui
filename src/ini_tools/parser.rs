use ini::{Ini, Properties};
use log::{error, trace, warn};
use std::{
    collections::HashMap,
    ffi::OsStr,
    fmt::Debug,
    io,
    path::{Path, PathBuf},
    str::ParseBoolError,
};

use crate::{
    get_cfg,
    ini_tools::writer::{remove_array, remove_entry, INI_SECTIONS},
    toggle_files,
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
    /// Do not use | no extra steps needed for validating a bool, .parse already handles validation or ParseBoolError
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
                trace!(
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

pub trait Valitidity {
    fn is_setup(&self) -> bool;
}

impl Valitidity for Ini {
    fn is_setup(&self) -> bool {
        INI_SECTIONS.iter().all(|section| {
            let trimmed_section: String = section.trim_matches(|c| c == '[' || c == ']').to_owned();
            self.section(Some(trimmed_section)).is_some()
        })
    }
}

#[derive(Default)]
pub struct RegMod {
    pub name: String,
    pub state: bool,
    pub files: Vec<PathBuf>,
}

impl RegMod {
    pub fn collect(path: &Path, skip_validation: bool) -> Result<Vec<Self>, ini::Error> {
        type HashData<'a> = HashMap<&'a str, (Result<bool, ParseBoolError>, Vec<PathBuf>)>;
        fn sync_keys<'a>(ini: &'a Ini, path: &Path) -> Result<HashData<'a>, ini::Error> {
            fn collect_file_data(section: &Properties) -> HashMap<&str, Vec<&str>> {
                section
                    .iter()
                    .enumerate()
                    .filter(|(_, (k, _))| *k != "array[]")
                    .map(|(i, (k, v))| {
                        let paths = section
                            .iter()
                            .skip(i + 1)
                            .take_while(|(k, _)| *k == "array[]")
                            .map(|(_, v)| v)
                            .collect();
                        (k, if v == "array" { paths } else { vec![v] })
                    })
                    .collect()
            }
            fn combine_map_data<'a>(
                state_map: HashMap<&'a str, &str>,
                file_map: HashMap<&str, Vec<&str>>,
            ) -> HashMap<&'a str, (Result<bool, ParseBoolError>, Vec<PathBuf>)> {
                state_map
                    .iter()
                    .filter_map(|(&key, &state_str)| {
                        file_map.get(&key).map(|file_strs| {
                            (
                                key,
                                (
                                    state_str.parse::<bool>(),
                                    file_strs.iter().map(PathBuf::from).collect::<Vec<_>>(),
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
                remove_entry(path, Some("registered-mods"), key)?;
                warn!("\"{}\" has no matching files", key);
            }
            let invalid_files: Vec<_> = file_data
                .keys()
                .filter(|k| !state_data.contains_key(*k))
                .cloned()
                .collect();
            for key in invalid_files {
                if file_data.get(key).unwrap().len() > 1 {
                    remove_array(path, key)?;
                } else {
                    remove_entry(path, Some("mod-files"), key)?;
                }
                file_data.remove(key);
                warn!("\"{}\" has no matching state", key);
            }
            Ok(combine_map_data(state_data, file_data))
        }
        fn collect_data_unsafe(ini: &Ini) -> Vec<(&str, &str, Vec<&str>)> {
            let mod_state_data = ini.section(Some("registered-mods")).unwrap();
            let mod_files_data = ini.section(Some("mod-files")).unwrap();
            mod_files_data
                .iter()
                .enumerate()
                .filter(|(_, (k, _))| *k != "array[]")
                .map(|(i, (k, v))| {
                    let paths: Vec<&str> = mod_files_data
                        .iter()
                        .skip(i + 1)
                        .take_while(|(k, _)| *k == "array[]")
                        .map(|(_, v)| v)
                        .collect();
                    let s = mod_state_data.get(k).unwrap();
                    (k, s, if v == "array" { paths } else { vec![v] })
                })
                .collect()
        }
        let ini = get_cfg(path).unwrap();

        if skip_validation {
            let parsed_data = collect_data_unsafe(&ini);
            Ok(parsed_data
                .iter()
                .map(|(n, s, f)| RegMod {
                    name: n.replace('_', " ").to_string(),
                    state: s.parse::<bool>().unwrap(),
                    files: f.iter().map(PathBuf::from).collect(),
                })
                .collect())
        } else {
            let parsed_data = sync_keys(&ini, path)?;
            Ok(parsed_data
                .iter()
                .filter_map(|(k, (s, f))| match &s {
                    Ok(bool) => match f.len() {
                        1 => {
                            match f[0]
                                .to_owned()
                                .validate(&ini, Some("mod-files"), skip_validation)
                            {
                                Ok(path) => Some(RegMod {
                                    name: k.replace('_', " ").to_string(),
                                    state: *bool,
                                    files: vec![path],
                                }),
                                Err(err) => {
                                    error!("Error: {}", err);
                                    None
                                }
                            }
                        }
                        2.. => match f
                            .to_owned()
                            .validate(&ini, Some("mod-files"), skip_validation)
                        {
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
                    },
                    Err(err) => {
                        error!("Error: {}", err);
                        None
                    }
                })
                .collect())
        }
    }
    pub fn verify_state(&self, game_dir: &Path, ini_file: &Path) -> Result<(), ini::Error> {
        let off_state = OsStr::new("disabled");
        if (!self.state
            && self
                .files
                .iter()
                .any(|path| path.extension().expect("file with extention") != off_state))
            || (self.state
                && self
                    .files
                    .iter()
                    .any(|path| path.extension().expect("file with extention") == off_state))
        {
            warn!(
                "wrong file state for \"{}\" chaning file extentions",
                self.name
            );
            toggle_files(
                &self.name.replace(' ', "_"),
                game_dir,
                self.state,
                self.files.to_owned(),
                ini_file,
            )?
        }
        Ok(())
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

use ini::{Ini, Properties};
use log::{error, trace, warn};
use std::{
    collections::HashMap,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
    str::ParseBoolError,
};

use crate::{
    get_cfg, new_io_error, toggle_files,
    utils::ini::writer::{remove_array, remove_entry, INI_SECTIONS},
};

pub trait ValueType: Sized {
    type ParseError: std::fmt::Display;
    fn parse_str(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        skip_validation: bool,
    ) -> Result<Self, Self::ParseError>;
    #[allow(unused_variables)]
    fn validate(
        self,
        ini: &Ini,
        section: Option<&str>,
        disable: bool,
    ) -> Result<Self, Self::ParseError> {
        Ok(self)
    }
}

impl ValueType for bool {
    type ParseError = ParseBoolError;
    fn parse_str(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        _skip_validation: bool,
    ) -> Result<Self, Self::ParseError> {
        ini.get_from(section, key)
            .expect("Validated by IniProperty::is_valid")
            .to_lowercase()
            .parse::<bool>()
    }
}

impl ValueType for u32 {
    type ParseError = std::num::ParseIntError;
    fn parse_str(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        _skip_validation: bool,
    ) -> Result<Self, Self::ParseError> {
        ini.get_from(section, key)
            .expect("Validated by IniProperty::is_valid")
            .parse::<u32>()
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
        let parsed_value = PathBuf::from(
            ini.get_from(section, key)
                .expect("Validated by IniProperty::is_valid"),
        );
        if skip_validation {
            Ok(parsed_value)
        } else {
            parsed_value.validate(ini, section, skip_validation)
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
                let game_dir =
                    match IniProperty::<PathBuf>::read(ini, Some("paths"), "game_dir", false) {
                        Some(ini_property) => ini_property.value,
                        None => return new_io_error!(ErrorKind::NotFound, "game_dir is not valid"),
                    };
                match validate_file(&game_dir.join(&self)) {
                    Ok(()) => Ok(self),
                    Err(err) => Err(err),
                }
            } else {
                match validate_existance(&self) {
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
        let parsed_value = read_array(
            ini.section(section)
                .expect("Validated by IniProperty::is_valid"),
            key,
        );
        if skip_validation {
            Ok(parsed_value)
        } else {
            parsed_value.validate(ini, section, skip_validation)
        }
    }
    fn validate(
        self,
        ini: &Ini,
        _section: Option<&str>,
        disable: bool,
    ) -> Result<Self, Self::ParseError> {
        if !disable {
            let game_dir = match IniProperty::<PathBuf>::read(ini, Some("paths"), "game_dir", false)
            {
                Some(ini_property) => ini_property.value,
                None => return new_io_error!(ErrorKind::NotFound, "game_dir is not valid"),
            };
            if let Some(err) = self
                .iter()
                .find_map(|path| validate_file(&game_dir.join(path)).err())
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

fn validate_file(path: &Path) -> Result<(), io::Error> {
    if path.extension().is_none() {
        let input_file = path.to_string_lossy().to_string();
        let split = input_file.rfind('\\').unwrap_or(0);
        input_file
            .split_at(if split != 0 { split + 1 } else { split })
            .1
            .to_string();
        return new_io_error!(
            ErrorKind::InvalidInput,
            format!("\"{input_file}\" does not have an extention")
        );
    }
    validate_existance(path)
}

fn validate_existance(path: &Path) -> Result<(), io::Error> {
    match path.try_exists() {
        Ok(result) => {
            if result {
                Ok(())
            } else {
                new_io_error!(
                    ErrorKind::NotFound,
                    format!("Path: \"{}\" can not be found on machine", path.display())
                )
            }
        }
        Err(_) => new_io_error!(
            ErrorKind::PermissionDenied,
            format!(
                "Path \"{}\"'s existance can neither be confirmed nor denied",
                path.display()
            )
        ),
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
        match IniProperty::is_valid(ini, section, key, skip_validation) {
            Ok(value) => {
                trace!(
                    "Success: read key: \"{key}\" Section: \"{}\" from ini",
                    section.expect("Passed in section should be valid")
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
                        "Value stored in Section: \"{}\", Key: \"{key}\" is not valid",
                        section.expect("Passed in section should be valid")
                    )
                );
                error!("Error: {err}");
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
                    Err(err) => Err(format!("Error: {err}")),
                },
                false => Err(format!("Key: \"{key}\" not found in {ini:?}")),
            },
            None => Err(format!(
                "Section: \"{}\" not found in {ini:?}",
                section.expect("Passed in section should be valid")
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
    pub config_files: Vec<PathBuf>,
}

impl RegMod {
    pub fn collect(path: &Path, skip_validation: bool) -> Result<Vec<Self>, ini::Error> {
        type ModData<'a> = Vec<(&'a str, Result<bool, ParseBoolError>, Vec<PathBuf>)>;
        fn sync_keys<'a>(ini: &'a Ini, path: &Path) -> Result<ModData<'a>, ini::Error> {
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
            ) -> ModData<'a> {
                let mut mod_data = state_map
                    .iter()
                    .filter_map(|(&key, &state_str)| {
                        file_map.get(&key).map(|file_strs| {
                            (
                                key,
                                state_str.to_lowercase().parse::<bool>(),
                                file_strs.iter().map(PathBuf::from).collect::<Vec<_>>(),
                            )
                        })
                    })
                    .collect::<ModData>();
                mod_data.sort_by_key(|(key, _, _)| *key);
                mod_data
            }
            let mod_state_data = ini
                .section(Some("registered-mods"))
                .expect("Validated by Ini::is_setup on startup");
            let mod_files_data = ini
                .section(Some("mod-files"))
                .expect("Validated by Ini::is_setup on startup");
            let mut state_data = mod_state_data.iter().collect::<HashMap<&str, &str>>();
            let mut file_data = collect_file_data(mod_files_data);
            let invalid_state = state_data
                .keys()
                .filter(|k| !file_data.contains_key(*k))
                .cloned()
                .collect::<Vec<_>>();
            for key in invalid_state {
                state_data.remove(key);
                remove_entry(path, Some("registered-mods"), key)?;
                warn!("\"{key}\" has no matching files");
            }
            let invalid_files = file_data
                .keys()
                .filter(|k| !state_data.contains_key(*k))
                .cloned()
                .collect::<Vec<_>>();
            for key in invalid_files {
                if file_data.get(key).expect("key exists").len() > 1 {
                    remove_array(path, key)?;
                } else {
                    remove_entry(path, Some("mod-files"), key)?;
                }
                file_data.remove(key);
                warn!("\"{key}\" has no matching state");
            }
            Ok(combine_map_data(state_data, file_data))
        }
        fn collect_data_unsafe(ini: &Ini) -> Vec<(&str, &str, Vec<&str>)> {
            let mod_state_data = ini
                .section(Some("registered-mods"))
                .expect("Validated by Ini::is_setup on startup");
            let mod_files_data = ini
                .section(Some("mod-files"))
                .expect("Validated by Ini::is_setup on startup");
            mod_files_data
                .iter()
                .enumerate()
                .filter(|(_, (k, _))| *k != "array[]")
                .map(|(i, (k, v))| {
                    let paths = mod_files_data
                        .iter()
                        .skip(i + 1)
                        .take_while(|(k, _)| *k == "array[]")
                        .map(|(_, v)| v)
                        .collect::<Vec<_>>();
                    let s = mod_state_data.get(k).expect("key exists");
                    (k, s, if v == "array" { paths } else { vec![v] })
                })
                .collect()
        }
        let ini = get_cfg(path).expect("Validated by Ini::is_setup on startup");

        if skip_validation {
            let parsed_data = collect_data_unsafe(&ini);
            Ok(parsed_data
                .iter()
                .map(|(n, s, f)| {
                    let (config_files, files) =
                        split_out_config_files(f.iter().map(PathBuf::from).collect());
                    RegMod {
                        name: n.to_string(),
                        state: s.to_lowercase().parse::<bool>().unwrap_or(true),
                        files,
                        config_files,
                    }
                })
                .collect())
        } else {
            let parsed_data = sync_keys(&ini, path)?;
            Ok(parsed_data
                .iter()
                .filter_map(|(k, s, f)| match &s {
                    Ok(bool) => match f.len() {
                        0 => unreachable!(),
                        1 => {
                            match f[0]
                                .to_owned()
                                .validate(&ini, Some("mod-files"), skip_validation)
                            {
                                Ok(path) => {
                                    let (config_files, files) = split_out_config_files(vec![path]);
                                    Some(RegMod {
                                        name: k.to_string(),
                                        state: *bool,
                                        files,
                                        config_files,
                                    })
                                }
                                Err(err) => {
                                    error!("Error: {err}");
                                    remove_entry(path, Some("registered-mods"), k)
                                        .expect("Key is valid");
                                    None
                                }
                            }
                        }
                        2.. => match f
                            .to_owned()
                            .validate(&ini, Some("mod-files"), skip_validation)
                        {
                            Ok(paths) => {
                                let (config_files, files) = split_out_config_files(paths);
                                Some(RegMod {
                                    name: k.to_string(),
                                    state: *bool,
                                    files,
                                    config_files,
                                })
                            }
                            Err(err) => {
                                error!("Error: {err}");
                                remove_entry(path, Some("registered-mods"), k)
                                    .expect("Key is valid");
                                None
                            }
                        },
                    },
                    Err(err) => {
                        error!("Error: {err}");
                        remove_entry(path, Some("registered-mods"), k).expect("Key is valid");
                        None
                    }
                })
                .collect())
        }
    }
    pub fn verify_state(&self, game_dir: &Path, ini_file: &Path) -> Result<(), ini::Error> {
        let off_state = "disabled";
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
            toggle_files(game_dir, self.state, self, Some(ini_file))?
        }
        Ok(())
    }
}
pub fn file_registered(mod_data: &[RegMod], files: &[PathBuf]) -> bool {
    files.iter().any(|path| {
        mod_data.iter().any(|registered_mod| {
            registered_mod.files.iter().any(|mod_file| path == mod_file)
                || registered_mod
                    .config_files
                    .iter()
                    .any(|mod_file| path == mod_file)
        })
    })
}

pub fn split_out_config_files(files: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    files
        .into_iter()
        .partition(|file| file.extension().expect("file with extention") == "ini")
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

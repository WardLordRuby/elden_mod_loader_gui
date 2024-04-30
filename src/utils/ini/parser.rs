use ini::{Ini, Properties};
use log::{error, trace, warn};
use std::{
    collections::HashMap,
    io::ErrorKind,
    path::{Path, PathBuf},
    str::ParseBoolError,
};

use crate::{
    get_cfg, new_io_error, toggle_files,
    utils::ini::{
        mod_loader::ModLoaderCfg,
        writer::{remove_array, remove_entry, INI_SECTIONS},
    },
    FileData, LOADER_SECTIONS, OFF_STATE,
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
        match ini
            .get_from(section, key)
            .expect("Validated by IniProperty::is_valid")
        {
            "0" => Ok(false),
            "1" => Ok(true),
            c => c.to_lowercase().parse::<bool>(),
        }
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
    type ParseError = std::io::Error;

    fn parse_str(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        skip_validation: bool,
    ) -> std::io::Result<Self> {
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

    fn validate(self, ini: &Ini, section: Option<&str>, disable: bool) -> std::io::Result<Self> {
        if !disable {
            if section == Some("mod-files") {
                let game_dir =
                    match IniProperty::<PathBuf>::read(ini, Some("paths"), "game_dir", false) {
                        Some(ini_property) => ini_property.value,
                        None => return new_io_error!(ErrorKind::NotFound, "game_dir is not valid"),
                    };
                validate_file(&game_dir.join(&self))?;
                Ok(self)
            } else {
                validate_existance(&self)?;
                Ok(self)
            }
        } else {
            Ok(self)
        }
    }
}

impl ValueType for Vec<PathBuf> {
    type ParseError = std::io::Error;

    fn parse_str(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        skip_validation: bool,
    ) -> std::io::Result<Self> {
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

    fn validate(self, ini: &Ini, _section: Option<&str>, disable: bool) -> std::io::Result<Self> {
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

fn validate_file(path: &Path) -> std::io::Result<()> {
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

fn validate_existance(path: &Path) -> std::io::Result<()> {
    match path.try_exists() {
        Ok(true) => Ok(()),
        Ok(false) => {
            new_io_error!(
                ErrorKind::NotFound,
                format!("Path: \"{}\" can not be found on machine", path.display())
            )
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
                    section.expect("Passed in section not valid")
                );
                Some(IniProperty {
                    //section: section.map(String::from),
                    //key: key.to_string(),
                    value,
                })
            }
            Err(err) => {
                error!(
                    "{}",
                    format!(
                        "Value stored in Section: \"{}\", Key: \"{key}\" is not valid",
                        section.expect("Passed in section not valid")
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
                true => {
                    T::parse_str(ini, section, key, skip_validation).map_err(|err| err.to_string())
                }
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
    /// Key in snake_case
    pub name: String,

    /// true = enabled | false = disabled
    pub state: bool,

    /// files with extension `.dll` | also possible they end in `.dll.disabled`  
    /// saved as short paths with `game_dir` truncated
    pub mod_files: Vec<PathBuf>,

    /// files with extension `.ini`  
    /// saved as short paths with `game_dir` truncated
    pub config_files: Vec<PathBuf>,

    /// files with any extension other than `.dll` or `.ini`  
    /// saved as short paths with `game_dir` truncated
    pub other_files: Vec<PathBuf>,

    /// contains properties related to if a mod has a set load order
    pub order: LoadOrder,
}

#[derive(Default)]
pub struct LoadOrder {
    /// if one of `self.mod_files` has a set load_order
    pub set: bool,

    /// the index of the selected `.dll` within `self.mod_files`
    pub i: usize,

    /// current set value of `load_order`  
    /// `self.order.at` is stored as 0 index | front end uses 1 index
    pub at: usize,
}

impl RegMod {
    fn split_out_config_files(
        in_files: Vec<PathBuf>,
    ) -> (Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>) {
        let len = in_files.len();
        let mut mod_files = Vec::with_capacity(len);
        let mut config_files = Vec::with_capacity(len);
        let mut other_files = Vec::with_capacity(len);
        in_files.into_iter().for_each(|file| {
            match FileData::from(&file.to_string_lossy()).extension {
                ".dll" => mod_files.push(file),
                ".ini" => config_files.push(file),
                _ => other_files.push(file),
            }
        });
        (mod_files, config_files, other_files)
    }

    /// This function omits the population of the `order` field
    pub fn new(name: &str, state: bool, in_files: Vec<PathBuf>) -> Self {
        let (mod_files, config_files, other_files) = RegMod::split_out_config_files(in_files);
        RegMod {
            name: String::from(name),
            state,
            mod_files,
            config_files,
            other_files,
            order: LoadOrder::default(),
        }
    }

    /// This function populates all fields
    fn new_full(
        name: &str,
        state: bool,
        in_files: Vec<PathBuf>,
        parsed_order_val: &mut Vec<(String, usize)>,
    ) -> std::io::Result<Self> {
        let (mod_files, config_files, other_files) = RegMod::split_out_config_files(in_files);
        let mut order = LoadOrder::default();
        let dll_files = mod_files
            .iter()
            .map(|f| {
                let file_name = f.file_name().ok_or(String::from("Bad file name"));
                Ok(file_name?.to_string_lossy().replace(OFF_STATE, ""))
            })
            .collect::<Result<Vec<_>, String>>()
            .map_err(|err| std::io::Error::new(ErrorKind::InvalidData, err))?;
        for (i, dll) in dll_files.iter().enumerate() {
            if let Some(remove_i) = parsed_order_val.iter().position(|(k, _)| k == dll) {
                order.set = true;
                order.i = i;
                order.at = parsed_order_val.swap_remove(remove_i).1;
                break;
            }
        }
        Ok(RegMod {
            name: String::from(name),
            state,
            mod_files,
            config_files,
            other_files,
            order,
        })
    }

    pub fn collect(ini_path: &Path, skip_validation: bool) -> std::io::Result<Vec<Self>> {
        type ModData<'a> = Vec<(&'a str, Result<bool, ParseBoolError>, Vec<PathBuf>)>;

        fn sync_keys<'a>(ini: &'a Ini, ini_path: &Path) -> std::io::Result<ModData<'a>> {
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
                remove_entry(ini_path, Some("registered-mods"), key)?;
                warn!("\"{key}\" has no matching files");
            }

            let invalid_files = file_data
                .keys()
                .filter(|k| !state_data.contains_key(*k))
                .cloned()
                .collect::<Vec<_>>();

            for key in invalid_files {
                if file_data.get(key).expect("key exists").len() > 1 {
                    remove_array(ini_path, key)?;
                } else {
                    remove_entry(ini_path, Some("mod-files"), key)?;
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

        let ini = get_cfg(ini_path)?;

        if skip_validation {
            let parsed_data = collect_data_unsafe(&ini);
            Ok(parsed_data
                .iter()
                .map(|(n, s, f)| {
                    RegMod::new(
                        n,
                        s.to_lowercase().parse::<bool>().unwrap_or(true),
                        f.iter().map(PathBuf::from).collect(),
                    )
                })
                .collect())
        } else {
            let parsed_data = sync_keys(&ini, ini_path)?;
            let game_dir = IniProperty::<PathBuf>::read(&ini, Some("paths"), "game_dir", false)
                .ok_or(std::io::Error::new(
                    ErrorKind::InvalidData,
                    "Could not read \"game_dir\" from file",
                ))?
                .value;
            let mut load_order_parsed = ModLoaderCfg::read_section(&game_dir, LOADER_SECTIONS[1])
                .map_err(|err| std::io::Error::new(ErrorKind::InvalidData, err))?
                .parse_section()
                .map_err(|err| std::io::Error::new(ErrorKind::InvalidData, err))?;
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
                                    RegMod::new_full(k, *bool, vec![path], &mut load_order_parsed)
                                        .ok()
                                }
                                Err(err) => {
                                    error!("Error: {err}");
                                    remove_entry(ini_path, Some("registered-mods"), k)
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
                                RegMod::new_full(k, *bool, paths, &mut load_order_parsed).ok()
                            }
                            Err(err) => {
                                error!("Error: {err}");
                                remove_entry(ini_path, Some("registered-mods"), k)
                                    .expect("Key is valid");
                                None
                            }
                        },
                    },
                    Err(err) => {
                        error!("Error: {err}");
                        remove_entry(ini_path, Some("registered-mods"), k).expect("Key is valid");
                        None
                    }
                })
                .collect())
        }
    }

    pub fn verify_state(&self, game_dir: &Path, ini_path: &Path) -> std::io::Result<()> {
        if (!self.state && self.mod_files.iter().any(FileData::is_enabled))
            || (self.state && self.mod_files.iter().any(FileData::is_disabled))
        {
            warn!(
                "wrong file state for \"{}\" chaning file extentions",
                self.name
            );
            toggle_files(game_dir, self.state, self, Some(ini_path)).map(|_| ())?
        }
        Ok(())
    }

    pub fn file_refs(&self) -> Vec<&Path> {
        let mut path_refs = Vec::with_capacity(self.all_files_len());
        path_refs.extend(self.mod_files.iter().map(|f| f.as_path()));
        path_refs.extend(self.config_files.iter().map(|f| f.as_path()));
        path_refs.extend(self.other_files.iter().map(|f| f.as_path()));
        path_refs
    }

    pub fn add_other_files_to_files<'a>(&'a self, files: &'a [PathBuf]) -> Vec<&'a Path> {
        let mut path_refs = Vec::with_capacity(files.len() + self.other_files_len());
        path_refs.extend(files.iter().map(|f| f.as_path()));
        path_refs.extend(self.config_files.iter().map(|f| f.as_path()));
        path_refs.extend(self.other_files.iter().map(|f| f.as_path()));
        path_refs
    }

    pub fn all_files_len(&self) -> usize {
        self.mod_files.len() + self.config_files.len() + self.other_files.len()
    }

    pub fn other_files_len(&self) -> usize {
        self.config_files.len() + self.other_files.len()
    }
}
pub fn file_registered(mod_data: &[RegMod], files: &[PathBuf]) -> bool {
    files.iter().any(|path| {
        mod_data.iter().any(|registered_mod| {
            registered_mod
                .file_refs()
                .iter()
                .any(|mod_file| path == mod_file)
        })
    })
}

pub trait IntoIoError {
    fn into_io_error(self) -> std::io::Error;
}

impl IntoIoError for ini::Error {
    fn into_io_error(self) -> std::io::Error {
        match self {
            ini::Error::Io(err) => err,
            ini::Error::Parse(err) => std::io::Error::new(ErrorKind::InvalidData, err),
        }
    }
}

pub trait ErrorClone {
    fn clone_err(&self) -> std::io::Error;
}

impl ErrorClone for std::io::Error {
    fn clone_err(&self) -> std::io::Error {
        std::io::Error::new(self.kind(), self.to_string())
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

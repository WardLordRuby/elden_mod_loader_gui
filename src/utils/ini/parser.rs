use ini::{Ini, Properties};
use log::{error, trace, warn};
use std::{
    collections::HashMap,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use crate::{
    get_cfg, new_io_error, toggle_files,
    utils::ini::{
        mod_loader::ModLoaderCfg,
        writer::{remove_array, remove_entry},
    },
    FileData, ARRAY_KEY, ARRAY_VALUE, INI_KEYS, INI_SECTIONS, LOADER_SECTIONS, OFF_STATE,
};

pub trait Parsable: Sized {
    fn parse_str<T: AsRef<Path>>(
        ini: &Ini,
        section: Option<&str>,
        partial_path: Option<T>,
        key: &str,
        skip_validation: bool,
    ) -> std::io::Result<Self>;
}

impl Parsable for bool {
    fn parse_str<T: AsRef<Path>>(
        ini: &Ini,
        section: Option<&str>,
        _partial_path: Option<T>,
        key: &str,
        _skip_validation: bool,
    ) -> std::io::Result<Self> {
        match ini
            .get_from(section, key)
            .expect("Validated by IniProperty::is_valid")
        {
            "0" => Ok(false),
            "1" => Ok(true),
            c => c.to_lowercase().parse::<bool>().map_err(|err| err.into_io_error()),
        }
    }
}

impl Parsable for u32 {
    fn parse_str<T: AsRef<Path>>(
        ini: &Ini,
        section: Option<&str>,
        _partial_path: Option<T>,
        key: &str,
        _skip_validation: bool,
    ) -> std::io::Result<Self> {
        ini.get_from(section, key)
            .expect("Validated by IniProperty::is_valid")
            .parse::<u32>()
            .map_err(|err| err.into_io_error())
    }
}

impl Parsable for PathBuf {
    fn parse_str<T: AsRef<Path>>(
        ini: &Ini,
        section: Option<&str>,
        partial_path: Option<T>,
        key: &str,
        skip_validation: bool,
    ) -> std::io::Result<Self> {
        let parsed_value = PathBuf::from({
            let value = ini.get_from(section, key);
            if matches!(value, Some(ARRAY_VALUE)) {
                return new_io_error!(
                    ErrorKind::InvalidData,
                    "Invalid type found. Expected: Path, Found: Vec<Path>"
                );
            }
            value.expect("Validated by IniProperty::is_valid")
        });
        if skip_validation {
            return Ok(parsed_value);
        }
        parsed_value.as_path().validate(partial_path)?;
        Ok(parsed_value)
    }
}

impl Parsable for Vec<PathBuf> {
    fn parse_str<T: AsRef<Path>>(
        ini: &Ini,
        section: Option<&str>,
        partial_path: Option<T>,
        key: &str,
        skip_validation: bool,
    ) -> std::io::Result<Self> {
        fn read_array(section: &Properties, key: &str) -> Vec<PathBuf> {
            section
                .iter()
                .skip_while(|(k, _)| *k != key)
                .skip_while(|(k, _)| *k == key)
                .take_while(|(k, _)| *k == ARRAY_KEY)
                .map(|(_, v)| PathBuf::from(v))
                .collect()
        }
        if !matches!(ini.get_from(section, key), Some(ARRAY_VALUE)) {
            return new_io_error!(
                ErrorKind::InvalidData,
                "Invalid type found. Expected: Vec<Path>, Found: Path"
            );
        }
        let parsed_value = read_array(
            ini.section(section).expect("Validated by IniProperty::is_valid"),
            key,
        );
        if skip_validation {
            return Ok(parsed_value);
        }
        parsed_value.validate(partial_path)?;
        Ok(parsed_value)
    }
}

pub trait Valitidity {
    /// _full_paths_ are assumed to Point to directories, where as _partial_paths_ are assumed to point to files  
    /// if you want to validate a _partial_path_ you must supply the _path_prefix_
    fn validate<P: AsRef<Path>>(&self, partial_path: Option<P>) -> std::io::Result<()>;
}

impl<T: AsRef<Path>> Valitidity for T {
    fn validate<P: AsRef<Path>>(&self, partial_path: Option<P>) -> std::io::Result<()> {
        if let Some(prefix) = partial_path {
            validate_file(&prefix.as_ref().join(self))?;
            Ok(())
        } else {
            validate_existance(self.as_ref())?;
            Ok(())
        }
    }
}

impl<T: AsRef<Path>> Valitidity for [T] {
    fn validate<P: AsRef<Path>>(&self, partial_path: Option<P>) -> std::io::Result<()> {
        let mut add_errors = String::new();
        let mut init_err = std::io::Error::new(ErrorKind::WriteZero, "");
        self.iter().for_each(|f| {
            if let Err(err) = f.validate(partial_path.as_ref()) {
                if init_err.kind() == ErrorKind::WriteZero {
                    init_err = err;
                } else if add_errors.is_empty() {
                    add_errors = err.to_string()
                } else {
                    add_errors.push_str(&format!("\n{err}"))
                }
            }
        });
        if init_err.kind() != ErrorKind::WriteZero {
            if add_errors.is_empty() {
                return Err(init_err);
            }
            return Err(init_err.add_msg(add_errors));
        }
        Ok(())
    }
}

fn validate_file(path: &Path) -> std::io::Result<()> {
    if path.extension().is_none() {
        let input_file = path.to_string_lossy().to_string();
        let split = input_file.rfind('\\').unwrap_or(0);
        return new_io_error!(
            ErrorKind::InvalidInput,
            format!(
                "\"{}\" does not have an extention",
                input_file.split_at(if split != 0 { split + 1 } else { split }).1
            )
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

pub trait Setup {
    fn is_setup(&self, sections: &[Option<&str>]) -> bool;
    fn init_section(&mut self, section: Option<&str>) -> std::io::Result<()>;
}

impl Setup for Ini {
    fn is_setup(&self, sections: &[Option<&str>]) -> bool {
        sections.iter().all(|&section| self.section(section).is_some())
    }

    fn init_section(&mut self, section: Option<&str>) -> std::io::Result<()> {
        trace!(
            "Section: \"{}\" not found creating new",
            section.expect("Passed in section not valid")
        );
        self.with_section(section).set("setter_temp_val", "0");
        if self.delete_from(section, "setter_temp_val").is_none() {
            return new_io_error!(
                ErrorKind::BrokenPipe,
                format!("Failed to create a new section: \"{}\"", section.unwrap())
            );
        };
        Ok(())
    }
}

#[derive(Debug)]
pub struct IniProperty<T: Parsable> {
    //section: Option<String>,
    //key: String,
    pub value: T,
}

impl<T: Parsable> IniProperty<T> {
    pub fn read(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        skip_validation: bool,
    ) -> std::io::Result<IniProperty<T>> {
        Ok(IniProperty {
            //section: section.map(String::from),
            //key: key.to_string(),
            value: IniProperty::is_valid(ini, section, key, skip_validation)?,
        })
    }

    fn is_valid(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        skip_validation: bool,
    ) -> std::io::Result<T> {
        match &ini.section(section) {
            Some(s) => match s.contains_key(key) {
                true => {
                    // This will have to be abstracted to the caller if we want the ability for the caller to specify the _path_prefix_
                    // right now _game_dir_ is the only valid prefix && "mod-files" is the only place _short_paths_ are stored
                    let game_dir = if section == INI_SECTIONS[3] {
                        Some(
                            IniProperty::<PathBuf>::read(ini, INI_SECTIONS[1], INI_KEYS[1], false)?
                                .value,
                        )
                    } else {
                        None
                    };
                    T::parse_str(ini, section, game_dir, key, skip_validation)
                }
                false => new_io_error!(
                    ErrorKind::NotFound,
                    format!("Key: \"{key}\" not found in {ini:?}")
                ),
            },
            None => new_io_error!(
                ErrorKind::NotFound,
                format!(
                    "Section: \"{}\" not found in {ini:?}",
                    section.expect("Passed in section should be valid")
                )
            ),
        }
    }
}

#[derive(Default)]
pub struct RegMod {
    /// user defined Key in snake_case
    pub name: String,

    /// true = enabled | false = disabled
    pub state: bool,

    /// files associated with the Registered Mod
    pub files: SplitFiles,

    /// contains properties related to if a mod has a set load order
    pub order: LoadOrder,
}

#[derive(Default)]
pub struct SplitFiles {
    /// files with extension `.dll` | also possible they end in `.dll.disabled`  
    /// saved as short paths with `game_dir` truncated
    pub dll: Vec<PathBuf>,

    /// files with extension `.ini`  
    /// saved as short paths with `game_dir` truncated
    pub config: Vec<PathBuf>,

    /// files with any extension other than `.dll` or `.ini`  
    /// saved as short paths with `game_dir` truncated
    pub other: Vec<PathBuf>,
}

#[derive(Default)]
pub struct LoadOrder {
    /// if one of `SplitFiles.dll` has a set load_order
    pub set: bool,

    /// the index of the selected `mod_file` within `SplitFiles.dll`
    pub i: usize,

    /// current set value of `load_order`  
    /// `self.at` is stored as 0 index | front end uses 1 index
    pub at: usize,
}

impl LoadOrder {
    fn from(dll_files: &[PathBuf], parsed_order_val: &HashMap<String, usize>) -> Self {
        let mut order = LoadOrder::default();
        if dll_files.is_empty() {
            return order;
        }
        if let Some(files) = dll_files
            .iter()
            .map(|f| {
                let file_name = f.file_name();
                Some(file_name?.to_string_lossy().replace(OFF_STATE, ""))
            })
            .collect::<Option<Vec<_>>>()
        {
            for (i, dll) in files.iter().enumerate() {
                if let Some(v) = parsed_order_val.get(dll) {
                    order.set = true;
                    order.i = i;
                    order.at = *v;
                    break;
                }
            }
        } else {
            error!("Failed to retrieve file_name for Path in: {dll_files:?} Returning LoadOrder::default")
        };
        order
    }
}

impl SplitFiles {
    fn from(in_files: Vec<PathBuf>) -> Self {
        let len = in_files.len();
        let mut dll = Vec::with_capacity(len);
        let mut config = Vec::with_capacity(len);
        let mut other = Vec::with_capacity(len);
        in_files.into_iter().for_each(|file| {
            match FileData::from(&file.to_string_lossy()).extension {
                ".dll" => dll.push(file),
                ".ini" => config.push(file),
                _ => other.push(file),
            }
        });
        SplitFiles { dll, config, other }
    }

    /// returns references to all files
    pub fn file_refs(&self) -> Vec<&Path> {
        let mut path_refs = Vec::with_capacity(self.len());
        path_refs.extend(self.dll.iter().map(|f| f.as_path()));
        path_refs.extend(self.config.iter().map(|f| f.as_path()));
        path_refs.extend(self.other.iter().map(|f| f.as_path()));
        path_refs
    }

    /// returns references to `input_files` + `self.config` + `self.other`
    pub fn add_other_files_to_files<'a>(&'a self, files: &'a [PathBuf]) -> Vec<&'a Path> {
        let mut path_refs = Vec::with_capacity(files.len() + self.other_files_len());
        path_refs.extend(files.iter().map(|f| f.as_path()));
        path_refs.extend(self.config.iter().map(|f| f.as_path()));
        path_refs.extend(self.other.iter().map(|f| f.as_path()));
        path_refs
    }

    #[inline]
    /// total number of files
    pub fn len(&self) -> usize {
        self.dll.len() + self.config.len() + self.other.len()
    }

    #[inline]
    /// returns true if all fields contain no PathBufs
    pub fn is_empty(&self) -> bool {
        self.dll.is_empty() && self.config.is_empty() && self.other.is_empty()
    }

    #[inline]
    /// number of `config` and `other`
    pub fn other_files_len(&self) -> usize {
        self.config.len() + self.other.len()
    }
}

impl RegMod {
    /// this function omits the population of the `order` field
    pub fn new(name: &str, state: bool, in_files: Vec<PathBuf>) -> Self {
        RegMod {
            name: name.trim().replace(' ', "_"),
            state,
            files: SplitFiles::from(in_files),
            order: LoadOrder::default(),
        }
    }

    /// unlike `new` this function returns a `RegMod` with all fields populated  
    /// `parsed_order_val` can be obtained from `ModLoaderCfg::parse_section()`
    pub fn with_load_order(
        name: &str,
        state: bool,
        in_files: Vec<PathBuf>,
        parsed_order_val: &HashMap<String, usize>,
    ) -> Self {
        let split_files = SplitFiles::from(in_files);
        let load_order = LoadOrder::from(&split_files.dll, parsed_order_val);
        RegMod {
            name: name.trim().replace(' ', "_"),
            state,
            files: split_files,
            order: load_order,
        }
    }

    fn from_split_files(name: &str, state: bool, in_files: SplitFiles, order: LoadOrder) -> Self {
        RegMod {
            name: String::from(name),
            state,
            files: in_files,
            order,
        }
    }

    // MARK: FIXME?
    // when is the best time to verify parsed data? currently we verify data after shaping it
    // the code would most likely be cleaner if we verified it apon parsing before doing any shaping

    // should we have two collections? one for deserialization(full) one for just collect and verify
    pub fn collect(ini_path: &Path, skip_validation: bool) -> std::io::Result<Vec<Self>> {
        type CollectedMaps<'a> = (HashMap<&'a str, &'a str>, HashMap<&'a str, Vec<&'a str>>);
        type ModData<'a> = Vec<(
            &'a str,
            Result<bool, std::str::ParseBoolError>,
            SplitFiles,
            LoadOrder,
        )>;

        fn sync_keys<'a>(ini: &'a Ini, ini_path: &Path) -> std::io::Result<CollectedMaps<'a>> {
            fn collect_paths(section: &Properties) -> HashMap<&str, Vec<&str>> {
                section
                    .iter()
                    .enumerate()
                    .filter(|(_, (k, _))| *k != ARRAY_KEY)
                    .map(|(i, (k, v))| {
                        let paths = section
                            .iter()
                            .skip(i + 1)
                            .take_while(|(k, _)| *k == ARRAY_KEY)
                            .map(|(_, v)| v)
                            .collect();
                        (k, if v == ARRAY_VALUE { paths } else { vec![v] })
                    })
                    .collect()
            }

            let mod_state_data = ini
                .section(INI_SECTIONS[2])
                .expect("Validated by Ini::is_setup on startup");
            let dll_data = ini
                .section(INI_SECTIONS[3])
                .expect("Validated by Ini::is_setup on startup");
            let mut state_data = mod_state_data.iter().collect::<HashMap<&str, &str>>();
            let mut file_data = collect_paths(dll_data);
            let invalid_state = state_data
                .keys()
                .filter(|k| !file_data.contains_key(*k))
                .cloned()
                .collect::<Vec<_>>();

            for key in invalid_state {
                state_data.remove(key);
                remove_entry(ini_path, INI_SECTIONS[2], key)?;
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
                    remove_entry(ini_path, INI_SECTIONS[3], key)?;
                }
                file_data.remove(key);
                warn!("\"{key}\" has no matching state");
            }

            assert_eq!(state_data.len(), file_data.len());
            Ok((state_data, file_data))
        }

        fn combine_map_data<'a>(
            map_data: CollectedMaps<'a>,
            parsed_order_val: &HashMap<String, usize>,
        ) -> ModData<'a> {
            let mut count = 0_usize;
            let mut mod_data = map_data
                .0
                .iter()
                .filter_map(|(&key, &state_str)| {
                    map_data.1.get(&key).map(|file_strs| {
                        let split_files = SplitFiles::from(
                            file_strs.iter().map(PathBuf::from).collect::<Vec<_>>(),
                        );
                        let load_order = LoadOrder::from(&split_files.dll, parsed_order_val);
                        if load_order.set {
                            count += 1
                        }
                        (
                            key,
                            state_str.to_lowercase().parse::<bool>(),
                            split_files,
                            load_order,
                        )
                    })
                })
                .collect::<ModData>();

            // if this fails `sync_keys()` did not do its job
            assert_eq!(map_data.1.len(), mod_data.len());

            mod_data.sort_by_key(|(_, _, _, l)| if l.set { l.at } else { usize::MAX });
            mod_data[count..].sort_by_key(|(key, _, _, _)| *key);
            mod_data
        }

        fn collect_data_unchecked(ini: &Ini) -> Vec<(&str, &str, Vec<&str>)> {
            let mod_state_data = ini
                .section(INI_SECTIONS[2])
                .expect("Validated by Ini::is_setup on startup");
            let dll_data = ini
                .section(INI_SECTIONS[3])
                .expect("Validated by Ini::is_setup on startup");
            dll_data
                .iter()
                .enumerate()
                .filter(|(_, (k, _))| *k != ARRAY_KEY)
                .map(|(i, (k, v))| {
                    let paths = dll_data
                        .iter()
                        .skip(i + 1)
                        .take_while(|(k, _)| *k == ARRAY_KEY)
                        .map(|(_, v)| v)
                        .collect::<Vec<_>>();
                    let s = mod_state_data.get(k).expect("key exists");
                    (k, s, if v == ARRAY_VALUE { paths } else { vec![v] })
                })
                .collect()
        }

        let ini = get_cfg(ini_path)?;

        if skip_validation {
            let parsed_data = collect_data_unchecked(&ini);
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
            let game_dir =
                IniProperty::<PathBuf>::read(&ini, INI_SECTIONS[1], INI_KEYS[1], false)?.value;
            let load_order_parsed = ModLoaderCfg::read_section(&game_dir, LOADER_SECTIONS[1])?
                .parse_section()
                .map_err(|err| err.into_io_error())?;
            let parsed_data = combine_map_data(parsed_data, &load_order_parsed);
            let mut output = Vec::with_capacity(parsed_data.len());
            for (k, s, f, l) in parsed_data {
                match &s {
                    Ok(bool) => {
                        if let Err(err) = f.file_refs().validate(Some(&game_dir)) {
                            error!("Error: {err}");
                            remove_entry(ini_path, INI_SECTIONS[2], k).expect("Key is valid");
                        } else {
                            let reg_mod = RegMod::from_split_files(k, *bool, f, l);
                            // MARK: FIXME
                            // verify_state should be ran within collect, but this call is too late, we should handle verification earilier
                            // when sync keys hits an error we should give it a chance to correct by calling verify_state before it deletes an entry
                            reg_mod.verify_state(&game_dir, ini_path)?;
                            output.push(reg_mod)
                        }
                    }
                    Err(err) => {
                        error!("Error: {err}");
                        remove_entry(ini_path, INI_SECTIONS[2], k).expect("Key is valid");
                    }
                }
            }
            Ok(output)
        }
    }

    pub fn verify_state(&self, game_dir: &Path, ini_path: &Path) -> std::io::Result<()> {
        if (!self.state && self.files.dll.iter().any(FileData::is_enabled))
            || (self.state && self.files.dll.iter().any(FileData::is_disabled))
        {
            warn!(
                "wrong file state for \"{}\" chaning file extentions",
                self.name
            );
            let _ = toggle_files(game_dir, self.state, self, Some(ini_path))?;
        }
        Ok(())
    }
}

pub fn file_registered(mod_data: &[RegMod], files: &[PathBuf]) -> bool {
    files.iter().any(|path| {
        mod_data.iter().any(|registered_mod| {
            registered_mod
                .files
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

impl IntoIoError for std::str::ParseBoolError {
    fn into_io_error(self) -> std::io::Error {
        std::io::Error::new(ErrorKind::InvalidData, self.to_string())
    }
}

impl IntoIoError for std::num::ParseIntError {
    fn into_io_error(self) -> std::io::Error {
        std::io::Error::new(ErrorKind::InvalidData, self.to_string())
    }
}

pub trait ModError {
    fn add_msg(self, msg: String) -> std::io::Error;
}

impl ModError for std::io::Error {
    fn add_msg(self, msg: String) -> std::io::Error {
        std::io::Error::new(self.kind(), format!("{msg}\n\nError: {self}"))
    }
}

pub trait ErrorClone {
    fn clone_err(self) -> std::io::Error;
}

impl ErrorClone for &std::io::Error {
    fn clone_err(self) -> std::io::Error {
        std::io::Error::new(self.kind(), self.to_string())
    }
}

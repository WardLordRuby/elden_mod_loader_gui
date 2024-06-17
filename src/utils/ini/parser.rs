use ini::{Ini, Properties};
use std::{
    collections::{HashMap, HashSet},
    io::ErrorKind,
    path::{Path, PathBuf},
    str::ParseBoolError,
};
use tracing::{error, info, instrument, trace, warn};

use crate::{
    file_name_from_str, files_not_found, get_cfg, new_io_error, omit_off_state, toggle_files,
    toggle_name_state,
    utils::ini::{
        common::Config,
        writer::{remove_array, remove_entry, save_bool, save_path, save_paths},
    },
    Cfg, DisplayName, DisplayPaths, DisplayState, DisplayStrs, FileData, IntoIoError, Merge,
    ModError, OrderMap, ARRAY_KEY, ARRAY_VALUE, INI_KEYS, INI_SECTIONS, REQUIRED_GAME_FILES,
};

pub trait Parsable: Sized {
    fn parse_str(
        ini: &Ini,
        section: Option<&str>,
        partial_path: Option<&Path>,
        key: &str,
        skip_validation: bool,
    ) -> std::io::Result<Self>;
}

impl Parsable for bool {
    fn parse_str(
        ini: &Ini,
        section: Option<&str>,
        _partial_path: Option<&Path>,
        key: &str,
        _skip_validation: bool,
    ) -> std::io::Result<Self> {
        let str = ini
            .get_from(section, key)
            .expect("Validated by IniProperty::is_valid");
        parse_bool(str).map_err(|err| err.into_io_error(key, str))
    }
}

#[inline]
fn parse_bool(str: &str) -> Result<bool, ParseBoolError> {
    match str {
        "0" => Ok(false),
        "1" => Ok(true),
        c => c.to_lowercase().parse::<bool>(),
    }
}

impl Parsable for u32 {
    fn parse_str(
        ini: &Ini,
        section: Option<&str>,
        _partial_path: Option<&Path>,
        key: &str,
        _skip_validation: bool,
    ) -> std::io::Result<Self> {
        let str = ini
            .get_from(section, key)
            .expect("Validated by IniProperty::is_valid");
        str.parse::<u32>().map_err(|err| err.into_io_error(key, str))
    }
}

impl Parsable for PathBuf {
    fn parse_str(
        ini: &Ini,
        section: Option<&str>,
        partial_path: Option<&Path>,
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
        if key == INI_KEYS[2] {
            match files_not_found(&parsed_value, &REQUIRED_GAME_FILES) {
                Ok(not_found) => {
                    if !not_found.is_empty() {
                        return new_io_error!(
                            ErrorKind::NotFound,
                            format!(
                                "Could not verify the install directory of Elden Ring, the following files were not found: {}",
                                DisplayStrs(&not_found),
                            )
                        );
                    }
                }
                Err(err) => return Err(err),
            }
        }
        Ok(parsed_value)
    }
}

impl Parsable for Vec<PathBuf> {
    fn parse_str(
        ini: &Ini,
        section: Option<&str>,
        partial_path: Option<&Path>,
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
        if let Err(err_data) = parsed_value.validate(partial_path) {
            return Err(err_data.errors.merge(true));
        };
        Ok(parsed_value)
    }
}

trait Valitidity {
    /// _full_paths_ (stored as `PathBuf`) are assumed to Point to directories,  
    /// where as _partial_paths_ (stored as `Vec<PathBuf>`) are assumed to point to files  
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

struct ValitidityError {
    error_paths: Vec<PathBuf>,
    errors: Vec<std::io::Error>,
}

trait ValitidityMany {
    /// _full_paths_ (stored as `PathBuf`) are assumed to Point to directories,  
    /// where as _partial_paths_ (stored as `Vec<PathBuf>`) are assumed to point to files  
    /// if you want to validate a _partial_path_ you must supply the _path_prefix_
    fn validate<P: AsRef<Path>>(&self, partial_path: Option<P>) -> Result<(), ValitidityError>;
}

impl<T: AsRef<Path>> ValitidityMany for [T] {
    fn validate<P: AsRef<Path>>(&self, partial_path: Option<P>) -> Result<(), ValitidityError> {
        let mut errors = Vec::new();
        let mut error_paths = Vec::new();
        self.iter().for_each(|f| {
            if let Err(err) = f.validate(partial_path.as_ref()) {
                errors.push(err);
                error_paths.push(f.as_ref().into());
            }
        });
        if !errors.is_empty() {
            return Err(ValitidityError {
                errors,
                error_paths,
            });
        }
        Ok(())
    }
}

#[instrument(level = "trace", skip_all)]
fn validate_file(path: &Path) -> std::io::Result<()> {
    if path.extension().is_none() {
        let input_file = path.to_string_lossy().to_string();
        return new_io_error!(
            ErrorKind::InvalidInput,
            format!(
                "\"{}\" does not have an extention",
                file_name_from_str(&input_file)
            )
        );
    }
    trace!(file = ?path.file_name().unwrap(), "has extension");
    validate_existance(path)
}

#[instrument(level = "trace", skip_all)]
fn validate_existance(path: &Path) -> std::io::Result<()> {
    match path.try_exists() {
        Ok(true) => {
            trace!(file = ?path.file_name().expect("valid directory"), "exists on disk");
            Ok(())
        }
        Ok(false) => {
            new_io_error!(
                ErrorKind::NotFound,
                format!(
                    "'{}' can not be found on machine",
                    file_name_from_str(path.to_str().unwrap_or_default())
                )
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
    fn is_setup(&self, sections: &[Option<&str>]) -> std::io::Result<ini::Ini>;
}

impl<T: AsRef<Path>> Setup for T {
    /// returns `Ok(ini)` if self is a path that:  
    /// - **exists** - if not returns `Err(NotFound)` or `Err(PermissionDenied)`  
    /// - **is .ini** - if not will panic!  
    /// - **contains all sections** - if not returns `Err(InvalidData)`  
    /// - **File::open** does not return an error  
    ///  
    /// it is safe to call unwrap on `get_cfg(self)` if this returns `Ok`
    #[instrument(level = "trace", name = "ini_is_setup", skip(self))]
    fn is_setup(&self, sections: &[Option<&str>]) -> std::io::Result<ini::Ini> {
        let file_data = self.as_ref().to_string_lossy();
        let file_data = FileData::from(&file_data);
        if file_data.extension != ".ini" {
            panic!("expected .ini found: {}", file_data.extension);
        }
        validate_existance(self.as_ref())?;
        let ini = get_cfg(self.as_ref())?;
        let not_found = sections
            .iter()
            .filter(|&&s| ini.section(s).is_none())
            .map(|s| s.expect("sections are always some"))
            .collect::<Vec<_>>();
        if !not_found.is_empty() {
            return new_io_error!(
                ErrorKind::InvalidData,
                format!(
                    "Could not find section(s): {}, in: {}",
                    DisplayStrs(&not_found),
                    self.as_ref()
                        .file_name()
                        .expect("valid file")
                        .to_str()
                        .unwrap_or_default()
                )
            );
        }
        trace!("ini found with all sections");
        Ok(ini)
    }
}

#[derive(Debug)]
pub struct IniProperty<T: Parsable> {
    //section: Option<String>,
    //key: String,
    pub value: T,
}

impl IniProperty<bool> {
    /// reads and parses a `bool` from a given Ini
    pub fn read(ini: &Ini, section: Option<&str>, key: &str) -> std::io::Result<IniProperty<bool>> {
        Ok(IniProperty {
            //section: section.map(String::from),
            //key: key.to_string(),
            value: IniProperty::is_valid(ini, section, key, false, None)?,
        })
    }
}
impl IniProperty<u32> {
    /// reads and parses a `u32` from a given Ini
    pub fn read(ini: &Ini, section: Option<&str>, key: &str) -> std::io::Result<IniProperty<u32>> {
        Ok(IniProperty {
            //section: section.map(String::from),
            //key: key.to_string(),
            value: IniProperty::is_valid(ini, section, key, false, None)?,
        })
    }
}
impl IniProperty<PathBuf> {
    /// reads, parses and optionally validates a `Pathbuf` from a given Ini  
    /// **Important:**
    /// - When reading a full length path, e.g. from Section: "paths", you _must not_ give a `path_prefix`  
    /// - When reading a partial path, e.g. from Section: "mod-files", you _must_ give a `path_prefix`  
    pub fn read(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        path_prefix: Option<&Path>,
        skip_validation: bool,
    ) -> std::io::Result<IniProperty<PathBuf>> {
        if section == INI_SECTIONS[1] && path_prefix.is_some() {
            panic!(
                "path_prefix is invalid when reading a path from: {}",
                INI_SECTIONS[1].unwrap()
            );
        } else if section == INI_SECTIONS[3] && path_prefix.is_none() {
            panic!(
                "path_prefix is required when reading a path from: {}",
                INI_SECTIONS[3].unwrap()
            );
        }
        Ok(IniProperty {
            //section: section.map(String::from),
            //key: key.to_string(),
            value: IniProperty::is_valid(ini, section, key, skip_validation, path_prefix)?,
        })
    }
}

impl IniProperty<Vec<PathBuf>> {
    /// reads, parses and optionally validates a `Vec<PathBuf>` from a given Ini
    pub fn read<P: AsRef<Path>>(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        path_prefix: P,
        skip_validation: bool,
    ) -> std::io::Result<IniProperty<Vec<PathBuf>>> {
        Ok(IniProperty {
            //section: section.map(String::from),
            //key: key.to_string(),
            value: IniProperty::is_valid(
                ini,
                section,
                key,
                skip_validation,
                Some(path_prefix.as_ref()),
            )?,
        })
    }
}

impl<T: Parsable> IniProperty<T> {
    fn is_valid(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        skip_validation: bool,
        path_prefix: Option<&Path>,
    ) -> std::io::Result<T> {
        match &ini.section(section) {
            Some(s) => match s.contains_key(key) {
                true => T::parse_str(ini, section, path_prefix, key, skip_validation),
                false => new_io_error!(
                    ErrorKind::NotFound,
                    format!("Key: \"{key}\" not found in ini.")
                ),
            },
            None => new_io_error!(
                ErrorKind::NotFound,
                format!(
                    "Section: \"{}\" not found in ini.",
                    section.expect("Passed in section should be valid")
                )
            ),
        }
    }
}

#[derive(Debug, Default)]
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

#[derive(Debug, Default)]
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

#[derive(Debug, Default)]
pub struct LoadOrder {
    /// if one of `SplitFiles.dll` has a set load_order
    pub set: bool,

    /// the index of the selected `mod_file` within `SplitFiles.dll`  
    /// derialization will set this to -1 if `set` is false and `SplitFiles.dll` is not len 1
    pub i: usize,

    /// current set value of `load_order`  
    /// `self.at` is stored as 0 index | front end uses 1 index
    pub at: usize,
}

impl LoadOrder {
    fn from(dll_files: &[PathBuf], parsed_order_val: &OrderMap) -> Self {
        if dll_files.is_empty() {
            return LoadOrder::default();
        }
        if let Some(files) = dll_files
            .iter()
            .map(|f| {
                let file_name = f.file_name();
                Some(String::from(omit_off_state(&file_name?.to_string_lossy())))
            })
            .collect::<Option<Vec<_>>>()
        {
            for (i, dll) in files.iter().enumerate() {
                if let Some(v) = parsed_order_val.get(dll) {
                    return LoadOrder {
                        set: true,
                        i,
                        at: *v,
                    };
                }
            }
        } else {
            error!(
                "Failed to retrieve file_name for Path in: {} Returning LoadOrder::default",
                DisplayPaths(dll_files)
            )
        };
        LoadOrder::default()
    }
}

fn get_correct_bucket<'a>(buckets: &'a mut SplitFiles, entry: &Path) -> &'a mut Vec<PathBuf> {
    let file_data = entry.to_string_lossy();
    let file_data = FileData::from(&file_data);
    match file_data.extension {
        ".ini" => &mut buckets.config,
        ".dll" => &mut buckets.dll,
        _ => &mut buckets.other,
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

    /// returns references to files in `self.dll`
    pub fn dll_refs(&self) -> Vec<&Path> {
        self.dll.iter().map(|f| f.as_path()).collect()
    }

    /// returns references to files in `self.config` and `self.other`
    pub fn other_file_refs(&self) -> Vec<&Path> {
        let mut path_refs = Vec::with_capacity(self.other_files_len());
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

    /// removes and returns entry using `swap_remove`
    fn remove(&mut self, path: &Path) -> Option<PathBuf> {
        let section = get_correct_bucket(self, path);
        if let Some(index) = section.iter().position(|f| f == path) {
            return Some(section.swap_remove(index));
        }
        None
    }

    /// adds a path to the correct field within `Self`
    pub fn add(&mut self, path: &Path) {
        let section = get_correct_bucket(self, path);
        section.push(PathBuf::from(path))
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
        parsed_order_val: &OrderMap,
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

    /// manual constructor for RegMod, note does not convert name to _snake_case_
    fn from_split_files(name: &str, state: bool, files: SplitFiles, order: LoadOrder) -> Self {
        RegMod {
            name: String::from(name),
            state,
            files,
            order,
        }
    }

    /// returns true if `Self` is _currently_ an array
    #[inline]
    pub fn is_array(&self) -> bool {
        self.files.len() > 1
    }

    /// verifies that files exist and recovers from the case where the file paths are saved in the  
    /// incorect state compaired to the name of the files currently saved on disk  
    ///
    /// then verifies that the saved state matches the state of the files  
    /// if not correct, runs toggle files to put them in the correct state  
    #[instrument(level = "trace", skip_all)]
    pub fn verify_state(&mut self, game_dir: &Path, ini_dir: &Path) -> std::io::Result<()> {
        fn count_try_verify_ouput(paths: &[PathBuf], game_dir: &Path) -> (usize, usize, usize) {
            let (mut exists, mut no_exist, mut errors) = (0_usize, 0_usize, 0_usize);
            paths.iter().for_each(|p| match game_dir.join(p).try_exists() {
                Ok(true) => exists += 1,
                Ok(false) => no_exist += 1,
                Err(_) => errors += 1,
            });
            (exists, no_exist, errors)
        }
        let (_, no_exist, errors) = count_try_verify_ouput(&self.files.dll, game_dir);
        if no_exist != 0 && errors == 0 {
            let alt_file_state = !FileData::state_data(&self.files.dll[0].to_string_lossy()).0;
            let test_alt_state = toggle_name_state(&self.files.dll, alt_file_state);
            let not_found = test_alt_state
                .iter()
                .zip(&self.files.dll)
                .filter_map(|(new, original)| {
                    if !matches!(game_dir.join(new).try_exists(), Ok(true)) {
                        Some(original.as_path())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            if not_found.is_empty() {
                let is_array = self.is_array();
                self.state = alt_file_state;
                self.files.dll = test_alt_state;
                self.write_to_file(ini_dir, is_array)?;
                info!(
                    "{}'s files were saved in the incorrect state, updated files to reflect the correct state: {}",
                    DisplayName(&self.name),
                    DisplayState(alt_file_state)
                );
                trace!(new_fnames = ?self.files.dll, "Recovered from Error: file names saved in the incorrect state")
            } else {
                return new_io_error!(
                    ErrorKind::NotFound,
                    format!(
                        "File(s): {}, can not be found on machine",
                        DisplayPaths(&not_found)
                    )
                );
            }
        } else if errors != 0 {
            return new_io_error!(
                ErrorKind::PermissionDenied,
                format!(
                    "One or more of: {}, existance can neither be confirmed nor denied",
                    DisplayPaths(&self.files.dll)
                )
            );
        }
        if (!self.state && self.files.dll.iter().any(FileData::is_enabled))
            || (self.state && self.files.dll.iter().any(FileData::is_disabled))
        {
            info!(
                "Wrong file state for \"{}\" chaning file state",
                DisplayName(&self.name)
            );
            return toggle_files(game_dir, self.state, self, Some(ini_dir));
        }
        trace!(fnames = ?self.files.dll, state = self.state, "verified");
        Ok(())
    }

    /// saves `self.state` and all `self.files` to file  
    /// it is important to keep track of the length of `self.files.file_refs()` before  
    /// making modifications to `self.files` to insure that the .ini file remains valid  
    pub fn write_to_file(&self, ini_dir: &Path, was_array: bool) -> std::io::Result<()> {
        save_bool(ini_dir, INI_SECTIONS[2], &self.name, self.state)?;
        let is_array = self.is_array();
        match (was_array, is_array) {
            (false, false) => save_path(
                ini_dir,
                INI_SECTIONS[3],
                &self.name,
                self.files.file_refs()[0],
            )?,
            (false, true) => save_paths(
                ini_dir,
                INI_SECTIONS[3],
                &self.name,
                &self.files.file_refs(),
            )?,
            (true, false) => {
                remove_array(ini_dir, &self.name)?;
                save_path(
                    ini_dir,
                    INI_SECTIONS[3],
                    &self.name,
                    self.files.file_refs()[0],
                )?
            }
            (true, true) => {
                remove_array(ini_dir, &self.name)?;
                save_paths(
                    ini_dir,
                    INI_SECTIONS[3],
                    &self.name,
                    &self.files.file_refs(),
                )?;
            }
        }
        Ok(())
    }

    /// removes `self` from the given ini_dir, removes files based on the current status of self.is_array()  
    /// note if you modify `self.files` you might run into unexpected behavior
    pub fn remove_from_file(&self, ini_dir: &Path) -> std::io::Result<()> {
        remove_entry(ini_dir, INI_SECTIONS[2], &self.name)?;
        if self.is_array() {
            remove_array(ini_dir, &self.name)?;
        } else {
            remove_entry(ini_dir, INI_SECTIONS[3], &self.name)?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct CollectedMods {
    pub mods: Vec<RegMod>,
    pub warnings: Option<std::io::Error>,
}

type CollectedMaps<'a> = (HashMap<&'a str, &'a str>, HashMap<&'a str, Vec<&'a str>>);

trait Combine {
    fn combine_map_data(
        self,
        parsed_order_val: Option<&OrderMap>,
        game_dir: &Path,
        ini_dir: &Path,
    ) -> CollectedMods;
}

impl<'a> Combine for CollectedMaps<'a> {
    #[instrument(level = "trace", skip_all)]
    fn combine_map_data(
        self,
        parsed_order_val: Option<&OrderMap>,
        game_dir: &Path,
        ini_dir: &Path,
    ) -> CollectedMods {
        type ModData<'a> = Vec<(&'a str, bool, SplitFiles, LoadOrder)>;

        let mut count = 0_usize;
        let mut warnings = Vec::new();
        let mut mod_data = self
            .0
            .iter()
            .filter_map(|(&key, &state_str)| {
                self.1.get(&key).map(|file_strs| {
                    let split_files =
                        SplitFiles::from(file_strs.iter().map(PathBuf::from).collect::<Vec<_>>());
                    let load_order = match parsed_order_val {
                        Some(data) => LoadOrder::from(&split_files.dll, data),
                        None => LoadOrder::default(),
                    };
                    if load_order.set {
                        count += 1
                    }
                    (
                        key,
                        parse_bool(state_str).unwrap_or(true),
                        split_files,
                        load_order,
                    )
                })
            })
            .collect::<ModData>();

        // if this fails `sync_keys()` did not do its job
        debug_assert_eq!(self.1.len(), mod_data.len());

        mod_data.sort_by_key(|(_, _, _, l)| if l.set { l.at } else { usize::MAX });
        mod_data[count..].sort_by_key(|(key, _, _, _)| *key);
        CollectedMods {
            mods: mod_data
                .drain(..)
                .filter_map(|d| {
                    let mut curr = RegMod::from_split_files(d.0, d.1, d.2, d.3);
                    if let Err(err) = curr.verify_state(game_dir, ini_dir) {
                        error!("{err}");
                        warnings.push(err);
                        if let Err(err) = curr.remove_from_file(ini_dir) {
                            error!("{err}");
                            warnings.push(err);
                        };
                        None
                    } else if let Err(mut err) =
                        curr.files.other_file_refs().validate(Some(&game_dir))
                    {
                        let mut can_continue = true;
                        let was_array = curr.is_array();
                        for i in (0..err.errors.len()).rev() {
                            if let Some(file) = curr.files.remove(&err.error_paths[i]) {
                                err.errors[i].add_msg(
                                    &format!(
                                    "File: '{}' was removed, and is no longer associated with: {}",
                                    file.display(),
                                    DisplayName(&curr.name)
                                ),
                                    true,
                                );
                                warn!("{}", err.errors[i]);
                                warnings.push(err.errors.pop().expect("valid range"))
                            } else {
                                err.errors.into_iter().for_each(|err| {
                                    error!("{err}");
                                    warnings.push(err);
                                });
                                if let Err(err) = curr.remove_from_file(ini_dir) {
                                    error!("{err}");
                                    warnings.push(err);
                                };
                                can_continue = false;
                                break;
                            }
                        }
                        if can_continue {
                            if let Err(err) = curr.write_to_file(ini_dir, was_array) {
                                error!("{err}");
                                None
                            } else {
                                Some(curr)
                            }
                        } else {
                            None
                        }
                    } else {
                        Some(curr)
                    }
                })
                .collect(),
            warnings: if warnings.is_empty() {
                None
            } else if warnings.len() == 1 {
                Some(warnings.remove(0))
            } else {
                Some(warnings.merge(true))
            },
        }
    }
}

impl Cfg {
    /// returns only valid mod data, if data was found to be invalid a message  
    /// is given to inform the user of why a mod was not included  
    ///
    /// validateds data in the following ways:
    /// - ensures data has both files and state associated with the same name  
    /// - `self.files.dll` are valid to exist on disk check `self.verify_state()` for how it can recover  
    /// - `self.files.other_file_refs()` are valid to exist on disk  
    ///   - if they are not files are removed and user can re-add them  
    #[instrument(level = "trace", skip(self, game_dir, include_load_order))]
    pub fn collect_mods<P: AsRef<Path>>(
        &self,
        game_dir: P,
        include_load_order: Option<&OrderMap>,
        skip_validation: bool,
    ) -> CollectedMods {
        if skip_validation {
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
            let parsed_data = collect_data_unchecked(self.data());
            return CollectedMods {
                mods: parsed_data
                    .iter()
                    .map(|(n, s, f)| {
                        RegMod::new(
                            n,
                            parse_bool(s).unwrap_or(true),
                            f.iter().map(PathBuf::from).collect(),
                        )
                    })
                    .collect(),
                warnings: None,
            };
        }

        let collected_mods =
            self.sync_keys()
                .combine_map_data(include_load_order, game_dir.as_ref(), self.path());
        trace!("collected {} mods", collected_mods.mods.len());
        collected_mods
    }

    /// parses the data associated with a given key into a `RegMod` if found  
    #[instrument(level = "trace", skip_all)]
    pub fn get_mod(
        &self,
        name: &slint::SharedString,
        game_dir: &Path,
        order_map: Option<&OrderMap>,
    ) -> std::io::Result<RegMod> {
        let key = name.replace(' ', "_");
        let split_files =
            if self
                .data()
                .get_from(INI_SECTIONS[3], &key)
                .ok_or(std::io::Error::new(
                    ErrorKind::InvalidInput,
                    format!("{key} not found in section: {}", INI_SECTIONS[3].unwrap()),
                ))?
                == ARRAY_VALUE
            {
                SplitFiles::from(
                    IniProperty::<Vec<PathBuf>>::read(
                        self.data(),
                        INI_SECTIONS[3],
                        &key,
                        game_dir,
                        false,
                    )?
                    .value,
                )
            } else {
                SplitFiles::from(vec![
                    IniProperty::<PathBuf>::read(
                        self.data(),
                        INI_SECTIONS[3],
                        &key,
                        Some(game_dir),
                        false,
                    )?
                    .value,
                ])
            };
        Ok(RegMod {
            order: if let Some(map) = order_map {
                LoadOrder::from(&split_files.dll, map)
            } else {
                LoadOrder::default()
            },
            state: IniProperty::<bool>::read(self.data(), INI_SECTIONS[2], &key)?.value,
            files: split_files,
            name: key,
        })
    }

    /// returns all the keys(as_lowercase) collected into a `Set`
    /// this also calls sync keys if invalid keys are found
    #[instrument(level = "trace", skip_all)]
    pub fn keys(&mut self) -> HashSet<String> {
        fn are_keys_ok(data: &ini::Ini) -> Option<HashSet<String>> {
            let reg_mods = data.section(INI_SECTIONS[2]).expect("Validated by is_setup");
            let mut keys = reg_mods.iter().map(|(k, _)| k.to_lowercase()).collect::<HashSet<_>>();
            let filtered_mod_files = data
                .section(INI_SECTIONS[3])
                .expect("Validated by is_setup")
                .iter()
                .filter_map(|(k, _)| if k != ARRAY_KEY { Some(k) } else { None })
                .collect::<Vec<_>>();
            match filtered_mod_files.iter().all(|k| !keys.insert(k.to_lowercase())) {
                true => Some(keys),
                false => None,
            }
        }

        if let Some(keys) = are_keys_ok(self.data()) {
            trace!("keys collected");
            return keys;
        }
        let registered_mods = {
            let (mods_map, _) = self.sync_keys();
            mods_map.keys().map(|k| k.to_lowercase()).collect::<HashSet<_>>()
        };
        self.update().expect("already exists in an accessable directory");
        registered_mods
    }

    /// returns all the registered file (as _short_paths_) in a `HashSet`
    // we _need_ to compare short_paths for the intened functionality to be correct
    // this is because mods typically have the same file names but in seprate directories
    pub fn files(&self) -> HashSet<&str> {
        let mod_files = self.data().section(INI_SECTIONS[3]).expect("Validated by is_setup");
        mod_files
            .iter()
            .filter_map(|(_, v)| if v != ARRAY_VALUE { Some(v) } else { None })
            .collect::<HashSet<_>>()
    }

    #[instrument(level = "trace", skip_all)]
    fn sync_keys(&self) -> CollectedMaps {
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

        let mod_state_data = self
            .data()
            .section(INI_SECTIONS[2])
            .expect("Validated by Ini::is_setup on startup");
        let dll_data = self
            .data()
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
            remove_entry(self.path(), INI_SECTIONS[2], key)
                .expect("Key is valid & ini has already been read");
            warn!(
                "{} has no registered files, mod was removed",
                DisplayName(key)
            );
        }

        let invalid_files = file_data
            .keys()
            .filter(|k| !state_data.contains_key(*k))
            .cloned()
            .collect::<Vec<_>>();

        for key in invalid_files {
            if file_data.get(key).expect("key exists").len() > 1 {
                remove_array(self.path(), key).expect("Key is valid & ini has already been read");
            } else {
                remove_entry(self.path(), INI_SECTIONS[3], key)
                    .expect("Key is valid & ini has already been read");
            }
            file_data.remove(key);
            warn!(
                "{} has no saved state data, mod was removed",
                DisplayName(key)
            );
        }

        debug_assert_eq!(state_data.len(), file_data.len());
        (state_data, file_data)
    }
}

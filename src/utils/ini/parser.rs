use ini::{Ini, Properties};
use log::{error, warn};
use std::{
    collections::HashMap,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use crate::{
    files_not_found, get_cfg, new_io_error, toggle_files, toggle_name_state,
    utils::ini::writer::{remove_array, remove_entry, save_bool, save_path, save_paths},
    Cfg, FileData, ARRAY_KEY, ARRAY_VALUE, INI_KEYS, INI_SECTIONS, OFF_STATE, REQUIRED_GAME_FILES,
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
        parse_bool(
            ini.get_from(section, key)
                .expect("Validated by IniProperty::is_valid"),
        )
    }
}

#[inline]
fn parse_bool(str: &str) -> std::io::Result<bool> {
    match str {
        "0" => Ok(false),
        "1" => Ok(true),
        c => c.to_lowercase().parse::<bool>().map_err(|err| err.into_io_error()),
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
        ini.get_from(section, key)
            .expect("Validated by IniProperty::is_valid")
            .parse::<u32>()
            .map_err(|err| err.into_io_error())
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
        if key == INI_KEYS[1] {
            match files_not_found(&parsed_value, &REQUIRED_GAME_FILES) {
                Ok(not_found) => {
                    if !not_found.is_empty() {
                        return new_io_error!(ErrorKind::NotFound, format!("Could not verify the install directory of Elden Ring, the following files were not found: \n{}", not_found.join("\n")));
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
            return Err(err_data.errors.merge());
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
    fn is_setup(&self, sections: &[Option<&str>]) -> std::io::Result<ini::Ini>;
}

impl<T: AsRef<Path>> Setup for T {
    /// returns `Ok(ini)` if self is a path that:  
    ///     _exists_ if not returns `Err(NotFound)` or `Err(PermissionDenied)`  
    ///     _is .ini_ if not returns `Err(InvalidInput)`  
    ///     _File::open_ does not return an error  
    ///     _contains all sections_ if not returns `Err(InvalidData)`  
    ///  
    /// it is safe to call unwrap on `get_cfg(self)` if this returns `Ok`
    fn is_setup(&self, sections: &[Option<&str>]) -> std::io::Result<ini::Ini> {
        let file_data = self.as_ref().to_string_lossy();
        let file_data = FileData::from(&file_data);
        if file_data.extension == ".ini" {
            validate_existance(self.as_ref())?;
            let ini = get_cfg(self.as_ref())?;
            let not_found = sections
                .iter()
                .filter(|&&s| ini.section(s).is_none())
                .map(|s| s.unwrap())
                .collect::<Vec<_>>();
            if not_found.is_empty() {
                Ok(ini)
            } else {
                new_io_error!(
                    ErrorKind::InvalidData,
                    format!(
                        "Could not find section(s): {:?} in {}",
                        not_found,
                        self.as_ref().display()
                    )
                )
            }
        } else {
            new_io_error!(
                ErrorKind::InvalidInput,
                format!("expected .ini found {}", file_data.extension)
            )
        }
    }
}

#[derive(Debug)]
pub struct IniProperty<T: Parsable> {
    //section: Option<String>,
    //key: String,
    pub value: T,
}

impl IniProperty<bool> {
    pub fn read(ini: &Ini, section: Option<&str>, key: &str) -> std::io::Result<IniProperty<bool>> {
        Ok(IniProperty {
            //section: section.map(String::from),
            //key: key.to_string(),
            value: IniProperty::is_valid(ini, section, key, false, None)?,
        })
    }
}
impl IniProperty<u32> {
    pub fn read(ini: &Ini, section: Option<&str>, key: &str) -> std::io::Result<IniProperty<u32>> {
        Ok(IniProperty {
            //section: section.map(String::from),
            //key: key.to_string(),
            value: IniProperty::is_valid(ini, section, key, false, None)?,
        })
    }
}
impl IniProperty<PathBuf> {
    pub fn read(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        skip_validation: bool,
    ) -> std::io::Result<IniProperty<PathBuf>> {
        Ok(IniProperty {
            //section: section.map(String::from),
            //key: key.to_string(),
            value: IniProperty::is_valid(ini, section, key, skip_validation, None)?,
        })
    }
}

impl IniProperty<Vec<PathBuf>> {
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

    pub fn dll_refs(&self) -> Vec<&Path> {
        self.dll.iter().map(|f| f.as_path()).collect()
    }

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

    fn remove(&mut self, path: &Path) -> Option<PathBuf> {
        if let Some(index) = self.config.iter().position(|f| f == path) {
            return Some(self.config.swap_remove(index));
        } else if let Some(index) = self.other.iter().position(|f| f == path) {
            return Some(self.other.swap_remove(index));
        } else if let Some(index) = self.dll.iter().position(|f| f == path) {
            return Some(self.dll.swap_remove(index));
        }
        None
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

    pub fn verify_state(&mut self, game_dir: &Path, ini_path: &Path) -> std::io::Result<()> {
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
        if no_exist > 0 && errors == 0 {
            let alt_file_state = !FileData::state_data(&self.files.dll[0].to_string_lossy()).0;
            let test_alt_state = toggle_name_state(&self.files.dll, alt_file_state);
            if test_alt_state
                .iter()
                .all(|f| matches!(game_dir.join(f).try_exists(), Ok(true)))
            {
                let is_array = self.files.len() > 1;
                self.state = alt_file_state;
                self.files.dll = test_alt_state;
                self.write_to_file(ini_path, is_array)?;
            } else if errors != 0 {
                return new_io_error!(
                    ErrorKind::PermissionDenied,
                    format!(
                        "One or more of: \"{:?}\"'s existance can neither be confirmed nor denied",
                        self.files.dll
                    )
                );
            } else {
                return new_io_error!(
                    ErrorKind::NotFound,
                    format!(
                        "One or more of: \"{:?}\" can not be found on machine",
                        self.files.dll
                    )
                );
            }
        }
        if (!self.state && self.files.dll.iter().any(FileData::is_enabled))
            || (self.state && self.files.dll.iter().any(FileData::is_disabled))
        {
            warn!(
                "wrong file state for \"{}\" chaning file extentions",
                self.name
            );
            toggle_files(game_dir, self.state, self, Some(ini_path))?
        }
        Ok(())
    }

    pub fn write_to_file(&self, ini_path: &Path, was_array: bool) -> std::io::Result<()> {
        save_bool(ini_path, INI_SECTIONS[2], &self.name, self.state)?;
        let is_array = self.files.len() > 1;
        match (was_array, is_array) {
            (false, false) => save_path(
                ini_path,
                INI_SECTIONS[3],
                &self.name,
                self.files.file_refs()[0],
            )?,
            (false, true) => save_paths(
                ini_path,
                INI_SECTIONS[3],
                &self.name,
                &self.files.file_refs(),
            )?,
            (true, false) => {
                remove_array(ini_path, &self.name)?;
                save_path(
                    ini_path,
                    INI_SECTIONS[3],
                    &self.name,
                    self.files.file_refs()[0],
                )?
            }
            (true, true) => {
                remove_array(ini_path, &self.name)?;
                save_paths(
                    ini_path,
                    INI_SECTIONS[3],
                    &self.name,
                    &self.files.file_refs(),
                )?;
            }
        }
        Ok(())
    }
}

impl Cfg {
    pub fn collect_mods<P: AsRef<Path>>(
        &self,
        game_dir: P,
        include_load_order: Option<&HashMap<String, usize>>,
        skip_validation: bool,
    ) -> Vec<RegMod> {
        type CollectedMaps<'a> = (HashMap<&'a str, &'a str>, HashMap<&'a str, Vec<&'a str>>);
        type ModData<'a> = Vec<(&'a str, bool, SplitFiles, LoadOrder)>;

        fn sync_keys<'a>(ini: &'a Ini, ini_path: &Path) -> CollectedMaps<'a> {
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
                remove_entry(ini_path, INI_SECTIONS[2], key)
                    .expect("Key is valid & ini has already been read");
                warn!("\"{key}\" has no matching files");
            }

            let invalid_files = file_data
                .keys()
                .filter(|k| !state_data.contains_key(*k))
                .cloned()
                .collect::<Vec<_>>();

            for key in invalid_files {
                if file_data.get(key).expect("key exists").len() > 1 {
                    remove_array(ini_path, key).expect("Key is valid & ini has already been read");
                } else {
                    remove_entry(ini_path, INI_SECTIONS[3], key)
                        .expect("Key is valid & ini has already been read");
                }
                file_data.remove(key);
                warn!("\"{key}\" has no matching state");
            }

            assert_eq!(state_data.len(), file_data.len());
            (state_data, file_data)
        }

        fn combine_map_data(
            map_data: CollectedMaps,
            parsed_order_val: Option<&HashMap<String, usize>>,
            game_dir: &Path,
            ini_dir: &Path,
        ) -> Vec<RegMod> {
            let mut count = 0_usize;
            let mut mod_data = map_data
                .0
                .iter()
                .filter_map(|(&key, &state_str)| {
                    map_data.1.get(&key).map(|file_strs| {
                        let split_files = SplitFiles::from(
                            file_strs.iter().map(PathBuf::from).collect::<Vec<_>>(),
                        );
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
            assert_eq!(map_data.1.len(), mod_data.len());

            mod_data.sort_by_key(|(_, _, _, l)| if l.set { l.at } else { usize::MAX });
            mod_data[count..].sort_by_key(|(key, _, _, _)| *key);
            mod_data
                .drain(..)
                .filter_map(|d| {
                    let mut curr = RegMod::from_split_files(d.0, d.1, d.2, d.3);
                    if let Err(err) = curr.verify_state(game_dir, ini_dir) {
                        error!("{err}");
                        None
                    } else if let Err(mut err) =
                        curr.files.other_file_refs().validate(Some(&game_dir))
                    {
                        let mut can_continue = true;
                        let is_array = curr.files.len() > 1;
                        for i in (0..err.errors.len()).rev() {
                            if curr.files.remove(&err.error_paths[i]).is_some() {
                                err.errors[i].add_msg(&format!(
                                    "This file was removed, and is no longer associated with: {}",
                                    curr.name
                                ));
                                warn!("{}", err.errors.pop().expect("valid range"));
                            } else {
                                error!("Error: {}", err.errors.merge());
                                remove_entry(ini_dir, INI_SECTIONS[2], &curr.name)
                                    .expect("Key is valid & ini has already been read");
                                can_continue = false;
                                break;
                            }
                        }
                        if can_continue && curr.write_to_file(ini_dir, is_array).is_ok() {
                            Some(curr)
                        } else {
                            None
                        }
                    } else {
                        Some(curr)
                    }
                })
                .collect()
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

        if skip_validation {
            let parsed_data = collect_data_unchecked(&self.data);
            parsed_data
                .iter()
                .map(|(n, s, f)| {
                    RegMod::new(
                        n,
                        parse_bool(s).unwrap_or(true),
                        f.iter().map(PathBuf::from).collect(),
                    )
                })
                .collect()
        } else {
            let parsed_data = sync_keys(&self.data, &self.dir);
            // parse_section is non critical write error | read_section is also non critical write error
            combine_map_data(
                parsed_data,
                include_load_order,
                game_dir.as_ref(),
                &self.dir,
            )
        }
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
    #[inline]
    fn into_io_error(self) -> std::io::Error {
        std::io::Error::new(ErrorKind::InvalidData, self.to_string())
    }
}

impl IntoIoError for std::num::ParseIntError {
    #[inline]
    fn into_io_error(self) -> std::io::Error {
        std::io::Error::new(ErrorKind::InvalidData, self.to_string())
    }
}

pub trait ModError {
    fn add_msg(&mut self, msg: &str);
}

impl ModError for std::io::Error {
    #[inline]
    fn add_msg(&mut self, msg: &str) {
        std::mem::swap(
            self,
            &mut std::io::Error::new(self.kind(), format!("{self}\n{msg}")),
        )
    }
}

pub trait ErrorClone {
    fn clone_err(&self) -> std::io::Error;
}

impl ErrorClone for std::io::Error {
    #[inline]
    fn clone_err(&self) -> std::io::Error {
        std::io::Error::new(self.kind(), self.to_string())
    }
}

trait Merge {
    fn merge(&self) -> std::io::Error;
}
impl Merge for Vec<std::io::Error> {
    fn merge(&self) -> std::io::Error {
        let mut new_err: std::io::Error = self[0].clone_err();
        (1..self.len()).for_each(|i| new_err.add_msg(&self[i].to_string()));
        new_err
    }
}

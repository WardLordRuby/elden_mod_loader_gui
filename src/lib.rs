pub mod utils {
    pub mod installer;
    pub mod ini {
        pub mod common;
        pub mod mod_loader;
        pub mod parser;
        pub mod writer;
    }
}

use ini::Ini;
use tracing::{info, instrument, trace, warn};
use utils::ini::{
    common::{Cfg, Config},
    parser::{IniProperty, IntoIoError, RegMod, Setup},
    writer::{new_cfg, save_path},
};

use std::{
    collections::HashSet,
    io::ErrorKind,
    path::{Path, PathBuf},
};

// changing the order of any of the following consts would not be good
const DEFAULT_GAME_DIR: [&str; 6] = [
    "Program Files (x86)",
    "Steam",
    "steamapps",
    "common",
    "ELDEN RING",
    "Game",
];

pub const REQUIRED_GAME_FILES: [&str; 3] = [
    "eldenring.exe",
    "oo2core_6_win64.dll",
    "eossdk-win64-shipping.dll",
];

pub const OFF_STATE: &str = ".disabled";

pub const INI_NAME: &str = "EML_gui_config.ini";
pub const INI_SECTIONS: [Option<&str>; 4] = [
    Some("app-settings"),
    Some("paths"),
    Some("registered-mods"),
    Some("mod-files"),
];
pub const INI_KEYS: [&str; 2] = ["dark_mode", "game_dir"];
pub const DEFAULT_INI_VALUES: [bool; 1] = [true];
pub const ARRAY_KEY: &str = "array[]";
pub const ARRAY_VALUE: &str = "array";

pub const LOADER_FILES: [&str; 3] = [
    "dinput8.dll.disabled",
    "dinput8.dll",
    "mod_loader_config.ini",
];
pub const LOADER_SECTIONS: [Option<&str>; 2] = [Some("modloader"), Some("loadorder")];
pub const LOADER_KEYS: [&str; 2] = ["load_delay", "show_terminal"];
pub const DEFAULT_LOADER_VALUES: [&str; 2] = ["5000", "0"];

#[macro_export]
macro_rules! new_io_error {
    ($kind:expr, $msg:expr) => {
        Err(std::io::Error::new($kind, $msg))
    };
}

pub struct PathErrors<'a> {
    pub ok_paths_short: Vec<&'a Path>,
    pub err_paths_long: Vec<&'a Path>,
}

impl PathErrors<'_> {
    fn new(size: usize) -> Self {
        PathErrors {
            ok_paths_short: Vec::with_capacity(size),
            err_paths_long: Vec::with_capacity(size),
        }
    }
}

/// returns `Ok(Vec<Path>)` if the remove path is a valid prefix of all input paths  
/// if not returns `Err(PathErrors)` that contains:
/// - `PathErrors.ok_paths_short` - sucessful strip_prefix() calls  
/// - `PathErrors.err_paths_long` - paths that remove path was not valid prefix  
#[instrument(level = "trace", skip_all)]
pub fn shorten_paths<'a, P: AsRef<Path>>(
    paths: &'a [P],
    remove: &P,
) -> Result<Vec<&'a Path>, PathErrors<'a>> {
    let mut results = PathErrors::new(paths.len());
    paths
        .iter()
        .for_each(|path| match path.as_ref().strip_prefix(remove) {
            Ok(shortened_path) => results.ok_paths_short.push(shortened_path),
            Err(_) => results.err_paths_long.push(path.as_ref()),
        });
    if results.err_paths_long.is_empty() {
        trace!("successfuly shortened all paths");
        Ok(results.ok_paths_short)
    } else {
        trace!(
            "unable to remove prefix on {} of {} paths",
            results.err_paths_long.len(),
            paths.len()
        );
        Err(results)
    }
}

/// Takes in a potential pathBuf, finds file_name name and outputs the new_state version
pub fn toggle_name_state(file_paths: &[PathBuf], new_state: bool) -> Vec<PathBuf> {
    file_paths
        .iter()
        .map(|path| {
            let file_name = match path.file_name() {
                Some(name) => name,
                None => path.as_os_str(),
            };
            let mut new_name = file_name.to_string_lossy().to_string();
            if let Some(index) = new_name.to_lowercase().find(OFF_STATE) {
                if new_state {
                    new_name.replace_range(index..index + OFF_STATE.chars().count(), "");
                }
            } else if !new_state {
                new_name.push_str(OFF_STATE);
            }
            let mut new_path = PathBuf::from(path);
            new_path.set_file_name(new_name);
            new_path
        })
        .collect()
}

/// toggle the state of the files saved in `reg_mod.files.dll`  
/// this function updates the reg_mod's modified files and state  
#[instrument(skip(game_dir, reg_mod, save_file), fields(name = reg_mod.name, prev_state = reg_mod.state))]
pub fn toggle_files(
    game_dir: &Path,
    new_state: bool,
    reg_mod: &mut RegMod,
    save_file: Option<&Path>,
) -> std::io::Result<()> {
    fn join_paths(base_path: &Path, join_to: &[PathBuf]) -> Vec<PathBuf> {
        join_to.iter().map(|path| base_path.join(path)).collect()
    }
    fn rename_files(
        num_files: &usize,
        paths: &[PathBuf],
        new_paths: &[PathBuf],
    ) -> std::io::Result<()> {
        if *num_files != paths.len() || *num_files != new_paths.len() {
            return new_io_error!(
                ErrorKind::InvalidInput,
                "Number of files and new paths must match"
            );
        }

        paths.iter().zip(new_paths.iter()).try_for_each(|(path, new_path)| {
            std::fs::rename(path, new_path)?;
            trace!(
                old = ?path.file_name().unwrap(),
                new = ?new_path.file_name().unwrap(), "Rename success"
            );
            Ok(())
        })
    }
    if reg_mod.state == new_state
        && reg_mod
            .files
            .dll
            .iter()
            .all(|f| FileData::state_data(&f.to_string_lossy()).0 == new_state)
    {
        trace!("Mod is already in the desired state");
        return Ok(());
    }

    let num_rename_files = reg_mod.files.dll.len();
    let is_array = reg_mod.files.len() > 1;

    let short_path_new = toggle_name_state(&reg_mod.files.dll, new_state);
    let full_path_new = join_paths(game_dir, &short_path_new);
    let full_path_original = join_paths(game_dir, &reg_mod.files.dll);

    rename_files(&num_rename_files, &full_path_original, &full_path_new)?;

    reg_mod.files.dll = short_path_new;
    reg_mod.state = new_state;
    info!(
        "{} {}",
        reg_mod.name,
        if reg_mod.state { "enabled" } else { "disabled" }
    );
    if let Some(file) = save_file {
        reg_mod.write_to_file(file, is_array)?
    }
    Ok(())
}

/// if cfg file does not exist or is not set up with provided sections this function will  
/// create a new ".ini" file in the given path  
#[instrument(skip_all, fields(cfg_dir = %from_path.display()))]
pub fn get_or_setup_cfg(from_path: &Path, sections: &[Option<&str>]) -> std::io::Result<Ini> {
    match from_path.is_setup(sections) {
        Ok(ini) => return Ok(ini),
        Err(err) => warn!(%err, "creating new"),
    }
    new_cfg(from_path)
}

/// returns ini read into memory, only call this if you know ini exists  
/// if you are not sure call `get_or_setup_cfg()` or `check &path.is_setup()`  
#[instrument(level = "trace", skip_all)]
pub fn get_cfg(from_path: &Path) -> std::io::Result<Ini> {
    let ini = Ini::load_from_file_noescape(from_path).map_err(|err| err.into_io_error("", ""))?;
    trace!(file = ?from_path.file_name().unwrap(), "loaded from file");
    Ok(ini)
}

pub enum Operation {
    All,
    Any,
    Count,
}

pub enum OperationResult<'a, T: ?Sized> {
    Bool(bool),
    Count((usize, HashSet<&'a T>)),
}

/// `Operation::All` and `Operation::Any` map to `OperationResult::bool(_result_)`  
/// `Operation::Count` maps to `OperationResult::Count((_num_found_, _HashSet<_&input_list_>))`  
/// when matching you will always have to `_ => unreachable()` for the return type you will never get
#[instrument(level = "trace", skip_all)]
pub fn does_dir_contain<'a, T>(
    dir: &Path,
    operation: Operation,
    list: &'a [&T],
) -> std::io::Result<OperationResult<'a, T>>
where
    T: std::borrow::Borrow<str> + std::cmp::Eq + std::hash::Hash + ?Sized,
    for<'b> &'b str: std::borrow::Borrow<T>,
{
    let entries = std::fs::read_dir(dir)?;
    let file_names = entries
        .filter_map(|entry| Some(entry.ok()?.file_name()))
        .collect::<Vec<_>>();
    let str_names = file_names.iter().filter_map(|f| f.to_str()).collect::<HashSet<_>>();

    match operation {
        Operation::All => Ok(OperationResult::Bool({
            let result = list.iter().all(|&check_file| str_names.contains(check_file));
            trace!(result, "all files found");
            result
        })),
        Operation::Any => Ok(OperationResult::Bool({
            let result = list.iter().any(|&check_file| str_names.contains(check_file));
            trace!(result, "any file found");
            result
        })),
        Operation::Count => {
            let collection = list
                .iter()
                .filter(|&check_file| str_names.contains(check_file))
                .copied()
                .collect::<HashSet<_>>();
            let num_found = collection.len();
            trace!(num_found, "files found");
            Ok(OperationResult::Count((num_found, collection)))
        }
    }
}

/// returns a collection of references to entries in list that are not found in the supplied directory  
/// returns an empty Vec if all files were found
pub fn files_not_found<'a, T>(dir: &Path, list: &'a [&T]) -> std::io::Result<Vec<&'a T>>
where
    T: std::borrow::Borrow<str> + std::cmp::Eq + std::hash::Hash + ?Sized,
    for<'b> &'b str: std::borrow::Borrow<T>,
{
    match does_dir_contain(dir, Operation::Count, list) {
        Ok(OperationResult::Count((c, _))) if c == list.len() => Ok(Vec::new()),
        Ok(OperationResult::Count((_, found_files))) => {
            Ok(list.iter().filter(|&&e| !found_files.contains(e)).copied().collect())
        }
        Err(err) => Err(err),
        _ => unreachable!(),
    }
}

pub struct FileData<'a> {
    pub name: &'a str,
    pub extension: &'a str,
    pub enabled: bool,
}

impl FileData<'_> {
    /// To get an accurate `FileData.name` function input needs `file_name()` called before hand  
    /// `FileData.extension` && `FileData.enabled` are accurate with any &Path str as input
    #[instrument(level = "trace", name = "file_data_from", skip_all)]
    pub fn from(name: &str) -> FileData {
        match FileData::state_data(name) {
            (false, index) => {
                let first_split = name.split_at(name[..index].rfind('.').expect("is file"));
                FileData {
                    name: first_split.0,
                    extension: first_split
                        .1
                        .split_at(first_split.1.rfind('.').expect("ends in .disabled"))
                        .0,
                    enabled: false,
                }
            }
            (true, _) => {
                let split = name.split_at(name.rfind('.').expect("is file"));
                FileData {
                    name: split.0,
                    extension: split.1,
                    enabled: true,
                }
            }
        }
    }

    /// index is only used in the _disabled_ state to locate where `OFF_STATE` begins  
    /// saftey check to make sure `OFF_STATE` is found at the end of a `&str`
    #[instrument(level = "trace")]
    pub fn state_data(path: &str) -> (bool, usize) {
        if let Some(index) = path.find(OFF_STATE) {
            let state = index != path.chars().count() - OFF_STATE.chars().count();
            trace!(correct_pos = !state, "{OFF_STATE} found");
            (state, index)
        } else {
            trace!("file not disabled");
            (true, 0)
        }
    }

    /// returns `true` if the file is in the enabled state  
    #[inline]
    #[instrument(level = "trace", skip_all)]
    pub fn is_enabled<T: AsRef<Path>>(path: &T) -> bool {
        FileData::state_data(&path.as_ref().to_string_lossy()).0
    }

    /// returns `true` if the file is in the disabled state  
    #[inline]
    #[instrument(level = "trace", skip_all)]
    pub fn is_disabled<T: AsRef<Path>>(path: &T) -> bool {
        !FileData::state_data(&path.as_ref().to_string_lossy()).0
    }
}

/// removes the off_state if the file name is in the off_state  
/// to get an accurate `FileData.name` function input needs `file_name()` called before hand  
#[instrument(level = "trace", skip_all)]
pub fn omit_off_state(name: &str) -> &str {
    let (file_state, index) = FileData::state_data(name);
    if file_state {
        name
    } else {
        &name[..index]
    }
}

/// convience function to map Option None to an io Error
#[inline]
pub fn parent_or_err(path: &Path) -> std::io::Result<&Path> {
    path.parent().ok_or(std::io::Error::new(
        ErrorKind::InvalidData,
        "Could not get parent_dir",
    ))
}
/// convience function to map Option None to an io Error
#[inline]
pub fn file_name_or_err(path: &Path) -> std::io::Result<&std::ffi::OsStr> {
    path.file_name().ok_or(std::io::Error::new(
        ErrorKind::InvalidData,
        "Could not get file_name",
    ))
}

pub enum PathResult {
    Full(PathBuf),
    Partial(PathBuf),
    None(PathBuf),
}

impl Cfg {
    /// returns various levels of a Path: "game_dir"  
    /// first tries to validate the path saved in the .ini if that fails then tries to located the "game_dir" on disk  
    /// if that fails will return a `PathResult::Partial` that is known to exist if not returns `PathResult::None` that contains just the found drive
    #[instrument(skip_all)]
    pub fn attempt_locate_game(&mut self) -> std::io::Result<PathResult> {
        if let Ok(path) =
            IniProperty::<PathBuf>::read(self.data(), INI_SECTIONS[1], INI_KEYS[1], false)
        {
            info!("Success: \"game_dir\" from ini is valid");
            return Ok(PathResult::Full(path.value));
        }
        let try_locate = attempt_locate_dir(&DEFAULT_GAME_DIR).unwrap_or("".into());
        if matches!(
            does_dir_contain(&try_locate, Operation::All, &REQUIRED_GAME_FILES),
            Ok(OperationResult::Bool(true))
        ) {
            info!("Success: located \"game_dir\" on drive");
            save_path(
                self.path(),
                INI_SECTIONS[1],
                INI_KEYS[1],
                try_locate.as_path(),
            )?;
            self.set(INI_SECTIONS[1], INI_KEYS[1], &try_locate.to_string_lossy());
            return Ok(PathResult::Full(try_locate));
        }
        if try_locate.components().count() > 1 {
            info!("Partial \"game_dir\" found");
            return Ok(PathResult::Partial(try_locate));
        }
        warn!("Could not locate \"game_dir\"");
        Ok(PathResult::None(try_locate))
    }
}

#[instrument(level = "trace", skip_all)]
fn attempt_locate_dir(target_path: &[&str]) -> std::io::Result<PathBuf> {
    let drive = get_current_drive()?;

    trace!(?drive, "Drive Found");

    match test_path_buf(PathBuf::from(&drive), target_path) {
        Ok(path) => Ok(path),
        Err(err) => {
            if &drive == "C:\\" {
                Err(err)
            } else {
                test_path_buf(PathBuf::from("C:\\"), target_path)
            }
        }
    }
}

#[instrument(level = "trace", skip_all)]
fn test_path_buf(mut path: PathBuf, target_path: &[&str]) -> std::io::Result<PathBuf> {
    for (index, dir) in target_path.iter().enumerate() {
        path.push(dir);
        trace!(path = %path.display(), "Testing");
        if !path.exists() && index > 1 {
            path.pop();
            break;
        } else if !path.exists() {
            return new_io_error!(
                ErrorKind::NotFound,
                format!("Could not locate {target_path:?}")
            );
        }
    }
    Ok(path)
}

fn get_current_drive() -> std::io::Result<std::ffi::OsString> {
    std::env::current_dir()?
        .components()
        .next()
        .map(|root| {
            let mut drive = root.as_os_str().to_ascii_uppercase();
            drive.push("\\");
            drive
        })
        .ok_or(std::io::Error::new(
            ErrorKind::InvalidData,
            "Could not get root component",
        ))
}

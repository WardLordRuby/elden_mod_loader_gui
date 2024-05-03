pub mod utils {
    pub mod installer;
    pub mod ini {
        pub mod mod_loader;
        pub mod parser;
        pub mod writer;
    }
}

use ini::Ini;
use log::{error, info, trace, warn};
use utils::ini::{
    parser::{IniProperty, IntoIoError, RegMod},
    writer::{remove_array, save_bool, save_path, save_paths},
};

use std::{
    collections::HashSet,
    io::ErrorKind,
    path::{Path, PathBuf},
};

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
pub const DEFAULT_INI_VALUES: [&str; 1] = ["true"];
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

pub struct PathErrors {
    pub ok_paths_short: Vec<PathBuf>,
    pub err_paths_long: Vec<PathBuf>,
}

impl PathErrors {
    fn new(size: usize) -> Self {
        PathErrors {
            ok_paths_short: Vec::with_capacity(size),
            err_paths_long: Vec::with_capacity(size),
        }
    }
}

pub fn shorten_paths(paths: &[PathBuf], remove: &PathBuf) -> Result<Vec<PathBuf>, PathErrors> {
    let mut results = PathErrors::new(paths.len());
    paths.iter().for_each(|path| match path.strip_prefix(remove) {
        Ok(file) => {
            results.ok_paths_short.push(PathBuf::from(file));
        }
        Err(_) => {
            results.err_paths_long.push(PathBuf::from(path));
        }
    });
    if results.err_paths_long.is_empty() {
        Ok(results.ok_paths_short)
    } else {
        Err(results)
    }
}

/// returns all the modified _partial_paths_
pub fn toggle_files(
    game_dir: &Path,
    new_state: bool,
    reg_mod: &RegMod,
    save_file: Option<&Path>,
) -> std::io::Result<Vec<PathBuf>> {
    /// Takes in a potential pathBuf, finds file_name name and outputs the new_state version
    fn toggle_name_state(file_paths: &[PathBuf], new_state: bool) -> Vec<PathBuf> {
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
                        new_name.replace_range(index..index + OFF_STATE.len(), "");
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
            Ok(())
        })
    }
    fn update_cfg(
        num_file: &usize,
        path_to_save: &[&Path],
        state: bool,
        key: &str,
        save_file: &Path,
    ) -> std::io::Result<()> {
        if *num_file == 1 {
            save_path(save_file, INI_SECTIONS[3], key, path_to_save[0])?;
        } else {
            remove_array(save_file, key)?;
            save_paths(save_file, key, path_to_save)?;
        }
        save_bool(save_file, INI_SECTIONS[2], key, state)?;
        Ok(())
    }
    let num_rename_files = reg_mod.files.dll.len();
    let num_total_files = num_rename_files + reg_mod.files.other_files_len();

    let file_paths = std::sync::Arc::new(reg_mod.files.dll.clone());
    let file_paths_clone = file_paths.clone();
    let game_dir_clone = game_dir.to_path_buf();

    let new_short_paths_thread =
        std::thread::spawn(move || toggle_name_state(&file_paths, new_state));
    let original_full_paths_thread =
        std::thread::spawn(move || join_paths(&game_dir_clone, &file_paths_clone));

    let short_path_new = new_short_paths_thread.join().unwrap_or(Vec::new());
    let all_short_paths = reg_mod.files.add_other_files_to_files(&short_path_new);
    let full_path_new = join_paths(game_dir, &short_path_new);
    let full_path_original = original_full_paths_thread.join().unwrap_or(Vec::new());

    rename_files(&num_rename_files, &full_path_original, &full_path_new)?;

    if save_file.is_some() {
        update_cfg(
            &num_total_files,
            &all_short_paths,
            new_state,
            &reg_mod.name,
            save_file.expect("is some"),
        )?;
    }
    Ok(short_path_new)
}

pub fn get_cfg(input_file: &Path) -> std::io::Result<Ini> {
    Ini::load_from_file_noescape(input_file).map_err(|err| err.into_io_error())
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
pub fn does_dir_contain<'a, T>(
    path: &Path,
    operation: Operation,
    list: &'a [&T],
) -> std::io::Result<OperationResult<'a, T>>
where
    T: std::borrow::Borrow<str> + std::cmp::Eq + std::hash::Hash + ?Sized,
    for<'b> &'b str: std::borrow::Borrow<T>,
{
    let entries = std::fs::read_dir(path)?;
    let file_names = entries
        .filter_map(|entry| Some(entry.ok()?.file_name()))
        .collect::<Vec<_>>();
    let str_names = file_names.iter().filter_map(|f| f.to_str()).collect::<HashSet<_>>();

    match operation {
        Operation::All => Ok(OperationResult::Bool(
            list.iter().all(|&check_file| str_names.contains(check_file)),
        )),
        Operation::Any => Ok(OperationResult::Bool(
            list.iter().any(|&check_file| str_names.contains(check_file)),
        )),
        Operation::Count => {
            let collection = list
                .iter()
                .filter(|&check_file| str_names.contains(check_file))
                .copied()
                .collect::<HashSet<_>>();
            Ok(OperationResult::Count((collection.len(), collection)))
        }
    }
}

pub struct FileData<'a> {
    pub name: &'a str,
    pub extension: &'a str,
    pub enabled: bool,
}

impl FileData<'_> {
    /// To get an accurate FileData.name function input needs .file_name() called before hand  
    /// FileData.extension && FileData.enabled are accurate with any &Path str as input
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

    #[inline]
    fn state_data(path: &str) -> (bool, usize) {
        if let Some(index) = path.find(OFF_STATE) {
            (index != path.len() - OFF_STATE.len(), index)
        } else {
            (true, 0)
        }
    }

    #[inline]
    pub fn is_enabled<T: AsRef<Path>>(path: &T) -> bool {
        FileData::state_data(&path.as_ref().to_string_lossy()).0
    }

    #[inline]
    pub fn is_disabled<T: AsRef<Path>>(path: &T) -> bool {
        !FileData::state_data(&path.as_ref().to_string_lossy()).0
    }
}

/// Convience function to map Option None to an io Error
pub fn parent_or_err(path: &Path) -> std::io::Result<&Path> {
    path.parent().ok_or(std::io::Error::new(
        ErrorKind::InvalidData,
        "Could not get parent_dir",
    ))
}
/// Convience function to map Option None to an io Error
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
pub fn attempt_locate_game(file_name: &Path) -> std::io::Result<PathResult> {
    let config: Ini = match get_cfg(file_name) {
        Ok(ini) => {
            trace!(
                "Success: (attempt_locate_game) Read ini from \"{}\"",
                file_name.display()
            );
            ini
        }
        Err(err) => {
            error!(
                "Failure: (attempt_locate_game) Could not complete. Could not read ini from \"{}\"",
                file_name.display()
            );
            error!("Error: {err}");
            return Ok(PathResult::None(PathBuf::new()));
        }
    };
    if let Ok(path) = IniProperty::<PathBuf>::read(&config, INI_SECTIONS[1], INI_KEYS[1], false)
        .and_then(|ini_property| {
            // right now all we do is log this error if we want to bail on the fn we can use the ? operator here
            match does_dir_contain(&ini_property.value, Operation::All, &REQUIRED_GAME_FILES) {
                Ok(OperationResult::Bool(true)) => Ok(ini_property.value),
                Ok(OperationResult::Bool(false)) => {
                    let err = format!(
                        "Required Game files not found in:\n\"{}\"",
                        ini_property.value.display()
                    );
                    error!("{err}");
                    new_io_error!(ErrorKind::NotFound, err)
                }
                Err(err) => {
                    error!("Error: {err}");
                    Err(err)
                }
                _ => unreachable!(),
            }
        })
    {
        info!("Success: \"game_dir\" from ini is valid");
        return Ok(PathResult::Full(path));
    }
    let try_locate = attempt_locate_dir(&DEFAULT_GAME_DIR).unwrap_or("".into());
    if matches!(
        does_dir_contain(&try_locate, Operation::All, &REQUIRED_GAME_FILES),
        Ok(OperationResult::Bool(true))
    ) {
        info!("Success: located \"game_dir\" on drive");
        save_path(
            file_name,
            INI_SECTIONS[1],
            INI_KEYS[1],
            try_locate.as_path(),
        )?;
        return Ok(PathResult::Full(try_locate));
    }
    if try_locate.components().count() > 1 {
        info!("Partial \"game_dir\" found");
        return Ok(PathResult::Partial(try_locate));
    }
    warn!("Could not locate \"game_dir\"");
    Ok(PathResult::None(try_locate))
}

fn attempt_locate_dir(target_path: &[&str]) -> std::io::Result<PathBuf> {
    let drive = get_current_drive()?;

    trace!("Drive Found: {:?}", &drive);

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

fn test_path_buf(mut path: PathBuf, target_path: &[&str]) -> std::io::Result<PathBuf> {
    for (index, dir) in target_path.iter().enumerate() {
        path.push(dir);
        trace!("Testing Path: {}", &path.display());
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
            let mut drive = root.as_os_str().to_os_string().to_ascii_uppercase();
            drive.push("\\");
            drive
        })
        .ok_or(std::io::Error::new(
            ErrorKind::InvalidData,
            "Could not get root component",
        ))
}

pub mod ini_tools {
    pub mod parser;
    pub mod writer;
}

use ini::Ini;
use ini_tools::{parser::IniProperty, writer::save_bool, writer::save_path};
use log::{debug, error, info, trace, warn};

use std::{
    env,
    ffi::{OsStr, OsString},
    fs::{read_dir, rename},
    path::{self, Path, PathBuf},
    rc::Rc,
};

use crate::ini_tools::writer::{remove_array, save_path_bufs};

pub const DEFAULT_GAME_DIR: [&str; 6] = [
    "Program Files (x86)",
    "Steam",
    "steamapps",
    "common",
    "ELDEN RING",
    "Game",
];
pub const CONFIG_DIR: &str = "test_files\\cfg.ini";
pub const REQUIRED_GAME_FILES: [&str; 3] = [
    "eldenring.exe",
    "oo2core_6_win64.dll",
    "eossdk-win64-shipping.dll",
];

pub fn shorten_paths(
    paths: Vec<PathBuf>,
    remove: &PathBuf,
) -> Result<Vec<PathBuf>, path::StripPrefixError> {
    paths
        .into_iter()
        .map(|path| path.strip_prefix(remove).map(|p| p.to_path_buf()))
        .collect()
}

// Make this fn more readable
pub fn toggle_files(
    key: &str,
    game_dir: &Path,
    new_state: bool,
    file_paths: Vec<PathBuf>,
) -> Result<&'static str, String> {
    fn toggle_name_state(file_name: &OsStr) -> OsString {
        let mut new_name = file_name.to_string_lossy().to_string();
        let new_name_clone = new_name.clone();
        if let Some(index) = new_name_clone.to_lowercase().find(".disabled") {
            new_name.replace_range(index..index + ".disabled".len(), "");
        } else {
            new_name.push_str(".disabled");
        }
        OsString::from(new_name)
    }

    let mut counter: usize = 0;
    let mut err_msg = String::new();
    let mut path_to_save = Vec::new();
    for path in file_paths.iter() {
        let full_path = game_dir.join(path);
        let mut collect_path = path.clone();
        let path_clone = full_path.clone();
        let mut new_path = full_path.clone();
        if new_path.pop() {
            collect_path.pop();
            if let Some(file_name) = path.file_name() {
                let new_name = toggle_name_state(file_name);
                new_path.push(new_name.clone());
                collect_path.push(new_name);
                match rename(&path_clone, &new_path) {
                    Ok(_) => {
                        counter += 1;
                        path_to_save.push(collect_path);
                    }
                    Err(err) => error!(
                        "File: {:?} into {:?} Error: {}",
                        &path_clone, &new_path, err
                    ),
                };
            };
        } else {
            err_msg = format!(
                "Error: Could not find parent directory at file_path array path: {:?}|",
                path
            );
        }
    }
    if counter == file_paths.len() {
        if counter == 1 {
            save_path(CONFIG_DIR, Some("mod-files"), key, &path_to_save[0]);
        } else {
            remove_array(CONFIG_DIR, key);
            save_path_bufs(CONFIG_DIR, key, &path_to_save);
        }
        save_bool(CONFIG_DIR, key, new_state);
        Ok("Success: All files in array have been renamed")
    } else {
        err_msg += "Error: Was not able to rename all files from array[file_paths]";
        Err(err_msg)
    }
}

pub fn get_cgf(input_file: &str) -> Result<Ini, ini::Error> {
    Ini::load_from_file_noescape(Path::new(input_file))
}

pub fn does_dir_contain(path: &Path, list: &[&str]) -> bool {
    match read_dir(path) {
        Ok(_) => {
            let file_names: Vec<_> = read_dir(path)
                .unwrap()
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.file_name())
                .filter_map(|file_name| file_name.to_str().map(String::from))
                .collect();

            let all_files_exist = list
                .iter()
                .all(|check_file| file_names.iter().any(|file_name| file_name == check_file));

            if all_files_exist {
                info!("Success: Directory verified");
                true
            } else {
                warn!(
                    "Failure: {:?} not found in: \"{}\"",
                    list,
                    path.to_string_lossy()
                );
                false
            }
        }
        Err(err) => {
            error!("Error::{} on reading directory", err);
            false
        }
    }
}

pub enum PathResult {
    Full(PathBuf),
    Partial(PathBuf),
    None(PathBuf),
}
pub fn attempt_locate_game(file_name: &str) -> PathResult {
    let config: Ini = match get_cgf(file_name) {
        Ok(ini) => {
            trace!(
                "Success: (attempt_locate_game) Read ini from \"{}\"",
                file_name
            );
            ini
        }
        Err(err) => {
            error!(
                "Failure: (attempt_locate_game) Could not complete. Could not read ini from \"{}\"",
                file_name
            );
            error!("Error: {}", err);
            return PathResult::None(PathBuf::from(""));
        }
    };
    let try_read: Option<PathBuf> =
        match IniProperty::<PathBuf>::read(&config, Some("paths"), "game_dir") {
            Ok(ini_property) => match does_dir_contain(&ini_property.value, &REQUIRED_GAME_FILES) {
                true => {
                    info!("Success: \"game_dir\" from ini is valid");
                    Some(ini_property.value)
                }
                false => {
                    warn!("Game files not found in directory read from ini");
                    None
                }
            },
            Err(err) => {
                error!("{}", err);
                None
            }
        };
    if let Some(path) = try_read {
        return PathResult::Full(path);
    }
    let try_locate = attempt_locate_dir(&DEFAULT_GAME_DIR).unwrap_or_else(|| "".into());
    if does_dir_contain(&try_locate, &REQUIRED_GAME_FILES) {
        info!("Success: located \"game_dir\" on drive");
        save_path(CONFIG_DIR, Some("paths"), "game_dir", try_locate.as_path());
        return PathResult::Full(try_locate);
    }
    if try_locate.components().count() > 1 {
        info!("Partial \"game_dir\" found");
        return PathResult::Partial(try_locate);
    }
    warn!("Could not locate \"game_dir\"");
    PathResult::None(try_locate)
}

pub fn attempt_locate_dir(target_path: &[&str]) -> Option<PathBuf> {
    let drive: String = match get_current_drive() {
        Some(drive) => drive,
        None => {
            info!("Failed to find find current Drive. Using 'C:\\'");
            "C:\\".to_string()
        }
    };
    let drive_ref: Rc<str> = Rc::from(drive.clone());
    info!("Drive Found: {}", drive_ref);

    match test_path_buf(PathBuf::from(drive), target_path) {
        Some(path) => Some(path),
        None => {
            if &*drive_ref == "C:\\" {
                None
            } else {
                test_path_buf(PathBuf::from("C:\\"), target_path)
            }
        }
    }
}

fn test_path_buf(mut path: PathBuf, target_path: &[&str]) -> Option<PathBuf> {
    for (index, folder) in target_path.iter().enumerate() {
        path.push(folder);
        info!("Testing Path: {:?}", &path);
        if !path.exists() && index > 1 {
            path.pop();
            break;
        } else if !path.exists() {
            return None;
        }
    }
    Some(path)
}

fn get_current_drive() -> Option<String> {
    let current_path = match env::current_dir() {
        Ok(path) => Some(path),
        Err(err) => {
            error!("{:?}", err);
            None
        }
    };
    current_path
        .and_then(|path| {
            path.components().next().map(|root| {
                let mut drive = root.as_os_str().to_os_string();
                drive.push("\\");
                drive
            })
        })
        .and_then(|os_string| os_string.to_str().map(|drive| drive.to_uppercase()))
}

pub mod ini_tools {
    pub mod parser;
    pub mod writer;
}

use ini::Ini;
use ini_tools::{parser::IniProperty, writer::save_path};
use log::{error, info, warn};

use std::{
    ffi::{OsStr, OsString},
    fs::{read_dir, rename},
    path::{self, Path, PathBuf},
    {env, rc::Rc},
};

const DEFAULT_COMMON_DIR: [&str; 4] = ["Program Files (x86)", "Steam", "steamapps", "common"];
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

pub fn toggle_files(file_paths: Vec<PathBuf>) -> Result<&'static str, String> {
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
    for (index, path) in file_paths.iter().enumerate() {
        let path_clone = path.clone();
        let mut new_path = path.clone();
        if new_path.pop() {
            if let Some(file_name) = path.file_name() {
                new_path.push(toggle_name_state(file_name));
                match rename(&path_clone, &new_path) {
                    Ok(_) => counter += 1,
                    Err(err) => error!(
                        "File: {:?} into {:?} Error: {}",
                        &path_clone, &new_path, err
                    ),
                };
            };
        } else {
            err_msg = format!(
                "Error: Could not find parent directory at file_path array index: {}",
                index
            );
        }
    }
    // if array len == counter then Success output new array with modified names or output bool and save names to ini
    if counter == file_paths.len() {
        Ok("Success: All files in array have been renamed")
    } else {
        err_msg += "Error: Was not able to rename all files from array[file_paths]";
        Err(err_msg)
    }
}

pub fn get_cgf(input_file: &str) -> Option<Ini> {
    match Ini::load_from_file_noescape(Path::new(input_file)) {
        Ok(ini) => {
            info!("Success:Config file found at \"{}\"", &input_file);
            Some(ini)
        }
        Err(err) => {
            error!("Error::{:?}", err);
            None
        }
    }
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

pub fn attempt_locate_common(config: &mut Ini) -> PathBuf {
    let read_common: Option<PathBuf> =
        match IniProperty::<PathBuf>::read(config, Some("paths"), "common_dir") {
            Ok(ini_property) => {
                let mut test_file_read = ini_property.value.clone();
                test_file_read.pop();
                match test_path_buf(test_file_read, &["common"]) {
                    Some(_) => {
                        info!("Success: \"common\" from ini is valid");
                        Some(ini_property.value)
                    }
                    None => {
                        warn!("\"common\" not found in directory read from ini");
                        None
                    }
                }
            }
            Err(err) => {
                error!("{}", err);
                None
            }
        };
    if let Some(path) = read_common {
        path
    } else {
        let common_dir = attempt_locate_dir(&DEFAULT_COMMON_DIR).unwrap_or_else(|| "".into());
        save_path(
            config,
            CONFIG_DIR,
            Some("paths"),
            "common_dir",
            common_dir.as_path(),
        );
        info!("default \"common_dir\" wrote to cfg file");
        common_dir
    }
}

fn attempt_locate_dir(target_path: &[&str]) -> Option<PathBuf> {
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

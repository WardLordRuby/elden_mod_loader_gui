// Hides console on --release | cant read logs if console is hidden
//#![windows_subsystem = "windows"]

slint::include_modules!();

mod ini_parser;

use ini::{EscapePolicy, Ini, LineSeparator, WriteOption};
use ini_parser::IniProperty;
use log::{debug, error, info, warn};
use native_dialog::FileDialog;

use std::{
    ffi::{OsStr, OsString},
    fs::{read_dir, rename},
    path::{Path, PathBuf},
    {env, rc::Rc},
};

const INI_WRITE_OPTIONS: WriteOption = WriteOption {
    escape_policy: EscapePolicy::Nothing,
    line_separator: LineSeparator::SystemDefault,
    kv_separator: " = ",
};

const CONFIG_DIR: &str = "tests\\cfg.ini";
const DEFAULT_COMMON_DIR: [&str; 4] = ["Program Files (x86)", "Steam", "steamapps", "common"];
const REQUIRED_GAME_FILES: [&str; 3] = [
    "eldenring.exe",
    "oo2core_6_win64.dll",
    "eossdk-win64-shipping.dll",
];

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    env_logger::init();
    let game_dir: PathBuf;
    {
        let mut config = match get_cgf(CONFIG_DIR) {
            Some(ini) => ini,
            None => {
                warn!("Ini not found. Creating new Ini");
                let mut new_ini = Ini::new();
                //format with comments and placeholders in sections
                new_ini
                    .write_to_file_opt(CONFIG_DIR, INI_WRITE_OPTIONS)
                    .unwrap();
                attempt_locate_common(&mut new_ini);
                get_cgf(CONFIG_DIR).unwrap()
            }
        };

        game_dir = match IniProperty::<PathBuf>::new(&config, Some("paths"), "game_dir") {
            Ok(ini_property) => {
                if does_dir_contain(&ini_property.value, &REQUIRED_GAME_FILES) {
                    info!("Success: \"game_dir\" verified");
                    ini_property.value
                } else {
                    attempt_locate_common(&mut config)
                }
            }
            Err(err) => {
                error!("{}", err);
                attempt_locate_common(&mut config)
            }
        };
    }

    ui.set_filepath(game_dir.to_string_lossy().to_string().into());

    ui.on_select_file_location({
        let ui_handle = ui.as_weak();
        move || {
            let mut config = get_cgf(CONFIG_DIR).unwrap();
            let ui = ui_handle.unwrap();

            let user_path: Result<String, &'static str> = match get_user_folder(game_dir.as_path())
            {
                Ok(opt) => match opt {
                    Some(selected_path) => Ok(selected_path.to_string_lossy().to_string()),
                    None => Err("No Path Selected"),
                },
                Err(err) => {
                    error!("Error selecting path");
                    error!("{}", err);
                    Err("Error selecting path")
                }
            };

            match user_path {
                Ok(path) => {
                    info!("User Selected Path: \"{}\"", &path);
                    ui.set_filepath(path.clone().into());
                    // include test for \Game\..Before REQUIRED_GAME_FILES
                    match does_dir_contain(Path::new(&path), &REQUIRED_GAME_FILES) {
                        true => {
                            info!("Success: Files found, saving diretory");
                            config.with_section(Some("paths")).set("game_dir", path);
                            config
                                .write_to_file_opt(CONFIG_DIR, INI_WRITE_OPTIONS)
                                .unwrap();
                        }
                        false => warn!("Failure: Files not found"),
                    };
                }
                Err(err) => {
                    info!("{}", err);
                    ui.set_filepath(err.into());
                }
            }
        }
    });

    ui.on_select_files({
        move || {
            let mut config = get_cgf(CONFIG_DIR).unwrap();
            //only save if valid files are selected
            let mod_files: Result<Vec<PathBuf>, &'static str> = match get_user_files(Path::new(""))
            {
                Ok(opt) => {
                    save_path_bufs(&mut config, &opt);
                    Ok(opt)
                }
                Err(err) => {
                    error!("Error selecting path");
                    error!("{}", err);
                    Err("Error selecting path")
                }
            };
        }
    });

    /*
    let test_files = vec![
        PathBuf::from("tests\\.a.txt"),
        PathBuf::from("tests\\.b.txt"),
        PathBuf::from("tests\\.c.txt"),
        PathBuf::from("tests\\.d.txt"),
        PathBuf::from("tests\\.e.txt"),
    ];
    match toggle_files(test_files) {
        Ok(info) => info!("{}", info),
        Err(err) => error!("{}", err),
    }
    */

    ui.run()
}

fn save_path_bufs(config: &mut Ini, files: &[PathBuf]) {
    let save_paths = files
        .iter()
        .map(|path| path.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\r\narray[]=");
    config
        .with_section(Some("mod-paths"))
        .set("test_files", format!("mod_files\r\narray[]={}", save_paths));
    config
        .write_to_file_opt(CONFIG_DIR, INI_WRITE_OPTIONS)
        .unwrap();
    //remove game dir from each path before save
}

fn toggle_files(file_paths: Vec<PathBuf>) -> Result<&'static str, String> {
    //change to append .disabled to end of file name
    fn toggle_name_state(file_name: &OsStr) -> OsString {
        let mut new_name = file_name.to_string_lossy().to_string();
        let new_name_clone = new_name.clone();
        if let Some(first_char) = new_name_clone.chars().next() {
            match first_char {
                '.' => new_name = new_name[1..].to_string(),
                _ => new_name = format!(".{}", new_name),
            };
        };
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

fn get_cgf(input_file: &str) -> Option<Ini> {
    let path = Path::new(input_file);
    match Ini::load_from_file_noescape(path) {
        Ok(ini) => {
            info!("Config file found at \"{}\"", &input_file);
            Some(ini)
        }
        Err(err) => {
            error!("Error::{:?}", err);
            None
        }
    }
}

fn get_user_folder(path: &Path) -> Result<Option<PathBuf>, native_dialog::Error> {
    FileDialog::new().set_location(path).show_open_single_dir()
}

fn get_user_files(path: &Path) -> Result<Vec<PathBuf>, native_dialog::Error> {
    FileDialog::new()
        .set_location(path)
        .show_open_multiple_file()
}

fn does_dir_contain(path: &Path, list: &[&str]) -> bool {
    match read_dir(path) {
        Ok(entries) => {
            let mut counter: usize = 0;
            for entry in entries.flatten() {
                if list
                    .iter()
                    .any(|check_file| entry.file_name().to_str() == Some(check_file))
                {
                    counter += 1;
                    debug!(
                        "Found: {:?} in selected directory",
                        &entry.file_name().to_str().unwrap()
                    );
                }
            }
            if counter == list.len() {
                true
            } else {
                warn!(
                    "All files were not found in: \"{}\"",
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

fn attempt_locate_common(config: &mut Ini) -> PathBuf {
    let read_common: Option<PathBuf> =
        match IniProperty::<PathBuf>::new(config, Some("paths"), "common_dir") {
            Ok(ini_property) => {
                let mut test_file_read = ini_property.value.clone();
                test_file_read.pop();
                match test_path_buf(test_file_read, &["common"]) {
                    Some(_) => {
                        info!("path to \"common\" read from ini is valid");
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
        config
            .with_section(Some("paths"))
            .set("common_dir", &common_dir.to_string_lossy().to_string());
        config
            .write_to_file_opt(CONFIG_DIR, INI_WRITE_OPTIONS)
            .unwrap();
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

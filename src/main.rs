// Hides console on --release | cant read logs if console is hidden
//#![windows_subsystem = "windows"]

slint::include_modules!();

mod ini_parser;

use ini::Ini;
use ini_parser::IniProperty;
use log::{error, info, warn};
use native_dialog::FileDialog;
use std::path::{Path, PathBuf};
use std::{env, fs::read_dir, rc::Rc};

const CONFIG_DIR: &str = "tests\\cfg.ini";
const DEFAULT_COMMON_DIR: [&str; 4] = ["Program Files (x86)", "Steam", "steamapps", "common"];
const REQUIRED_GAME_FILES: [&str; 1] = ["test_config.ini"];

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    env_logger::init();

    let mut config = match get_cgf(CONFIG_DIR) {
        Some(ini) => ini,
        None => {
            warn!("Ini not found. Creating new Ini");
            let mut new_ini = Ini::new();
            new_ini.write_to_file(CONFIG_DIR).unwrap();
            attempt_locate_common(&mut new_ini);
            get_cgf(CONFIG_DIR).unwrap()
        }
    };

    let game_dir: PathBuf = match IniProperty::<PathBuf>::new(&config, Some("paths"), "game_dir") {
        Ok(ini_property) => {
            if does_dir_contain(&ini_property.value, &REQUIRED_GAME_FILES) {
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

    ui.set_filepath(game_dir.to_string_lossy().to_string().into());

    ui.on_select_file_location({
        let ui_handle = ui.as_weak();
        move || {
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
                    match does_dir_contain(Path::new(&path), &REQUIRED_GAME_FILES) {
                        true => {
                            info!("Sucess: Files found, saving diretory");
                            config.with_section(Some("paths")).set("game_dir", path);
                            config.write_to_file(CONFIG_DIR).unwrap();
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
    ui.run()
}

fn get_cgf(input_file: &str) -> Option<Ini> {
    let path = Path::new(input_file);
    match Ini::load_from_file(path) {
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
                    info!(
                        "Found: {:?} in selected directory",
                        &entry.file_name().to_str().unwrap()
                    );
                }
            }
            if counter == list.len() {
                info!("All files found in: \"{}\"", path.to_string_lossy());
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
        config.write_to_file(CONFIG_DIR).unwrap();
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

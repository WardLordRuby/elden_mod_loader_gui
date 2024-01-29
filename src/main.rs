// Hides console on --release | cant read logs if console is hidden
//#![windows_subsystem = "windows"]

slint::include_modules!();
use ini::Ini;
use log::{error, info, warn};
use native_dialog::FileDialog;
use std::path::{Path, PathBuf};
use std::{env, fs::read_dir, rc::Rc, str::FromStr};

struct IniProperty {
    section: Option<String>,
    key: String,
    value: Value,
}

enum Value {
    AsBool(bool),
    AsPath(PathBuf),
}

impl Value {
    fn str_to_bool(value: Option<&str>) -> bool {
        bool::from_str(value.unwrap()).unwrap()
    }

    fn str_to_path(value: Option<&str>) -> PathBuf {
        PathBuf::from(value.unwrap())
    }

    fn unwrap_bool(self) -> bool {
        if let Value::AsBool(bool) = self {
            bool
        } else {
            panic!("Unexpected variant");
        }
    }

    fn unwrap_path(self) -> PathBuf {
        if let Value::AsPath(path) = self {
            path
        } else {
            panic!("Unexpected variant");
        }
    }
}

impl IniProperty {
    fn new(
        ini: &Ini,
        section: Option<&str>,
        key: &str,
        expected_type: &str,
    ) -> Result<IniProperty, String> {
        let result_type: ValFromIni = match expected_type {
            "bool" => Ok(ValFromIni::IsBool(ini.get_from(section, key))),
            "path" => Ok(ValFromIni::IsPath(ini.get_from(section, key))),
            _ => Err("Not a valid type"),
        }?;
        match IniProperty::is_valid(ini, section, key, result_type) {
            true => {
                let value: Value = match expected_type {
                    "bool" => Value::AsBool(Value::str_to_bool(ini.get_from(section, key))),
                    "path" => Value::AsPath(Value::str_to_path(ini.get_from(section, key))),
                    _ => panic!("Unsupported type"),
                };
                Ok(IniProperty {
                    section: Some(section.unwrap().into()),
                    key: key.into(),
                    value,
                })
            }
            false => Err(format!(
                "Value stored in Section: \"{}\", Key: \"{}\" is not valid",
                section.unwrap(),
                key
            )),
        }
    }

    fn is_valid(ini: &Ini, section: Option<&str>, key: &str, expected_type: ValFromIni) -> bool {
        match &ini.section(section) {
            Some(s) => match s.contains_key(key) {
                true => match expected_type {
                    ValFromIni::IsBool(_) => match expected_type.parse_bool() {
                        Ok(_) => true,
                        Err(err) => {
                            error!("Error {}", err);
                            false
                        }
                    },
                    ValFromIni::IsPath(_) => match expected_type.parse_path() {
                        Ok(_) => true,
                        Err(err) => {
                            error!("Error {}", err);
                            false
                        }
                    },
                },
                false => {
                    warn!("Key: \"{}\" not found in {:?}", key, ini);
                    false
                }
            },
            None => {
                warn!("Section: \"{}\" not found in {:?}", section.unwrap(), ini);
                false
            }
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    env_logger::init();

    let mut config = match get_cgf("tests\\cfg.ini") {
        Some(ini) => ini,
        None => {
            warn!("Ini not found. Creating new Ini");
            let default_path =
                attempt_locate_dir(&["Program Files (x86)", "Steam", "steamapps", "common"])
                    .unwrap_or_else(|| "\\".into());
            let mut new_ini = Ini::new();
            new_ini
                .with_section(Some("paths"))
                .set("game_dir", &default_path.to_string_lossy().to_string());
            new_ini.write_to_file("tests\\cfg.ini").unwrap();
            info!("default directory wrote to cfg file");
            new_ini
        }
    };

    let game_dir: PathBuf = match IniProperty::new(&config, Some("paths"), "game_dir", "path") {
        Ok(ini_property) => {
            info!("Sucessfuly read {} from cfg.ini", ini_property.key);
            let test_path = Value::unwrap_path(ini_property.value);
            if does_dir_contain(&test_path, &["test_config.ini"]) {
                test_path
            } else {
                attempt_locate_dir(&["Program Files (x86)", "Steam", "steamapps", "common"])
                    .unwrap_or_else(|| "\\".into())
            }
        }
        Err(err) => {
            error!("{}", err);
            attempt_locate_dir(&["Program Files (x86)", "Steam", "steamapps", "common"])
                .unwrap_or_else(|| "\\".into())
        }
    };

    ui.set_filepath(game_dir.to_string_lossy().to_string().into());

    ui.on_select_file_location({
        let ui_handle = ui.as_weak();
        move || {
            let ui = ui_handle.unwrap();
            let user_path: String = match get_user_folder(game_dir.as_path()) {
                Ok(opt) => match opt {
                    Some(selected_path) => selected_path.to_string_lossy().to_string(),
                    None => "No Path Selected".to_string(),
                },
                Err(err) => {
                    error!("{:?}", err);
                    "Error selecting path".to_string()
                }
            };
            info!("User Selected Path: {}", &user_path);
            match does_dir_contain(Path::new(&user_path), &["test_config.ini"]) {
                true => {
                    info!("Sucess: Files found, saving diretory");
                    config
                        .with_section(Some("paths"))
                        .set("game_dir", &user_path);
                    config.write_to_file("tests\\cfg.ini").unwrap();
                }
                false => warn!("Failure: Files not found"),
            };
            ui.set_filepath(user_path.clone().into());
        }
    });
    ui.run()
}

enum ValFromIni<'a> {
    IsBool(Option<&'a str>),
    IsPath(Option<&'a str>),
}

impl<'a> ValFromIni<'a> {
    fn parse_bool(&'a self) -> Result<ValFromIni, &'static str> {
        todo!()
    }

    fn parse_path(&self) -> Result<ValFromIni, &'static str> {
        match self {
            ValFromIni::IsPath(Some(value)) => match Path::new(value).exists() {
                true => Ok(ValFromIni::IsPath(Some(*value))),
                false => Err("Read path not found"),
            },
            _ => Err("Path invalid"),
        }
    }
}

fn get_cgf(input_file: &str) -> Option<Ini> {
    let path = Path::new(input_file);
    match Ini::load_from_file(path) {
        Ok(ini) => {
            info!("Config file found at {:?}", &input_file);
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
                info!("All files found in selected directory");
                true
            } else {
                warn!("All files were not found in selected directory");
                false
            }
        }
        Err(err) => {
            error!("{}: on reading directory", err);
            false
        }
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

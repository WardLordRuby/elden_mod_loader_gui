// Hides console on --release | cant read logs if console is hidden
//#![windows_subsystem = "windows"]

slint::include_modules!();
use log::{info, warn};
use native_dialog::FileDialog;
use std::path::{Path, PathBuf};
use std::{env, rc::Rc};

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    env_logger::init();

    let default_common_path: PathBuf = match attempt_locate_common() {
        Some(path) => path,
        None => PathBuf::from("\\"),
    };
    ui.set_filepath(default_common_path.to_string_lossy().to_string().into());

    ui.on_select_file_location({
        let ui_handle = ui.as_weak();
        move || {
            let ui = ui_handle.unwrap();
            let user_path: String = match get_user_folder(default_common_path.as_path()) {
                Ok(opt) => match opt {
                    Some(selected_path) => selected_path.to_string_lossy().to_string(),
                    None => "No Path Selected".to_string(),
                },
                Err(err) => {
                    warn!("{:?}", err);
                    "Error selecting path".to_string()
                }
            };
            ui.set_filepath(user_path.clone().into());
            info!("User Selected Path: {}", user_path.clone());
        }
    });
    ui.run()
}

fn get_user_folder(path: &Path) -> Result<Option<PathBuf>, native_dialog::Error> {
    FileDialog::new().set_location(path).show_open_single_dir()
}

fn attempt_locate_common() -> Option<PathBuf> {
    let drive: String = match get_current_drive() {
        Some(drive) => drive,
        None => {
            info!("Failed to find find current Drive. Using 'C:\\'");
            "C:\\".to_string()
        }
    };
    let drive_ref: Rc<str> = Rc::from(drive.clone());
    info!("Drive Found: {}", drive_ref);

    let target_path = ["Program Files (x86)", "Steam", "steamapps", "common"];
    match test_path(PathBuf::from(drive), &target_path) {
        Some(path) => Some(path),
        None => {
            if &*drive_ref == "C:\\" {
                None
            } else {
                test_path(PathBuf::from("C:\\"), &target_path)
            }
        }
    }
}

fn test_path(mut path: PathBuf, list: &[&str]) -> Option<PathBuf> {
    for (index, folder) in list.iter().enumerate() {
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
            warn!("{:?}", err);
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

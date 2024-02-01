// Hides console on --release | cant read logs if console is hidden
//#![windows_subsystem = "windows"]

slint::include_modules!();

mod ini_tools {
    pub mod parser;
    pub mod writer;
}

use ini::Ini;
use ini_tools::{parser::IniProperty, writer::*};
use log::{error, info, warn};
use native_dialog::FileDialog;

use std::path::{Path, PathBuf};

use elden_mod_loader_gui::*;

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    env_logger::init();
    {
        let mut config = match get_cgf(CONFIG_DIR) {
            Some(ini) => ini,
            None => {
                warn!("Ini not found. Creating new Ini");
                new_cfg(CONFIG_DIR);
                let mut new_ini: Ini = get_cgf(CONFIG_DIR).unwrap();
                attempt_locate_common(&mut new_ini);
                new_ini
            }
        };

        let game_dir: PathBuf =
            match IniProperty::<PathBuf>::read(&config, Some("paths"), "game_dir") {
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

        ui.on_select_game_dir({
            let ui_handle = ui.as_weak();
            move || {
                // remember to handle unwrap errors like this one
                let mut config = get_cgf(CONFIG_DIR).unwrap();
                let ui = ui_handle.unwrap();

                let user_path: Result<String, &'static str> =
                    match get_user_folder(game_dir.as_path()) {
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
                        let try_path: PathBuf = if does_dir_contain(Path::new(&path), &["Game"]) {
                            PathBuf::from(&format!("{}\\Game", path))
                        } else {
                            PathBuf::from(&path)
                        };
                        match does_dir_contain(Path::new(&try_path), &REQUIRED_GAME_FILES) {
                            true => {
                                info!("Success: Files found, saving diretory");
                                save_path(
                                    &mut config,
                                    CONFIG_DIR,
                                    Some("paths"),
                                    "game_dir",
                                    &try_path,
                                );
                                ui.set_filepath(try_path.to_string_lossy().to_string().into());
                            }
                            false => {
                                let msg: &str = "Failure: Files not found";
                                warn!("{}", &msg);
                                ui.set_filepath(try_path.to_string_lossy().to_string().into());
                            }
                        };
                    }
                    Err(err) => {
                        info!("{}", err);
                        ui.set_filepath(err.into());
                    }
                }
            }
        });
    }

    ui.on_select_files({
        move || {
            // remember to handle unwrap errors like this one
            let mut config = get_cgf(CONFIG_DIR).unwrap();
            let game_dir: Result<PathBuf, String> =
                match IniProperty::<PathBuf>::read(&config, Some("paths"), "game_dir") {
                    Ok(ini_property) => {
                        if does_dir_contain(&ini_property.value, &REQUIRED_GAME_FILES) {
                            Ok(ini_property.value)
                        } else {
                            Err("Error: Select game directory before adding mod files".to_string())
                        }
                    }
                    Err(err) => Err(err),
                };
            match game_dir {
                Ok(valid_dir) => {
                    let mod_files: Result<Vec<PathBuf>, &'static str> =
                        match get_user_files(Path::new(&valid_dir)) {
                            Ok(opt) => match opt.len() {
                                0 => Err("No files selected"),
                                _ => Ok(opt),
                            },
                            Err(err) => {
                                error!("{}", err);
                                Err("Error selecting path")
                            }
                        };
                    match mod_files {
                        Ok(files) => match shorten_paths(files, &valid_dir) {
                            Ok(paths) => match paths.len() {
                                1 => save_path(
                                    &mut config,
                                    CONFIG_DIR,
                                    Some("mod-files"),
                                    "test_files_single_path",
                                    paths[0].as_path(),
                                ),
                                _ => save_path_bufs(
                                    &mut config,
                                    CONFIG_DIR,
                                    "test_files_array",
                                    &paths,
                                ),
                            },
                            Err(err) => error!("Error: {}", err),
                        },
                        Err(err) => error!("{}", err),
                    }
                }
                Err(err) => error!("Error: {}", err),
            }
        }
    });

    ui.run()
}

fn get_user_folder(path: &Path) -> Result<Option<PathBuf>, native_dialog::Error> {
    FileDialog::new().set_location(path).show_open_single_dir()
}

fn get_user_files(path: &Path) -> Result<Vec<PathBuf>, native_dialog::Error> {
    FileDialog::new()
        .set_location(path)
        .show_open_multiple_file()
}

// Hides console on --release | cant read logs if console is hidden
//#![windows_subsystem = "windows"]

slint::include_modules!();

mod ini_tools {
    pub mod parser;
    pub mod writer;
}

use ini::Ini;
use ini_tools::{parser::RegMod, writer::*};
use log::{error, info, warn};
use native_dialog::FileDialog;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

use elden_mod_loader_gui::*;
fn main() -> Result<(), slint::PlatformError> {
    env_logger::init();
    let ui = App::new()?;
    let mut game_dir: PathBuf;
    let mut game_verified: bool;
    let mut reg_mods = RegMod::collect(CONFIG_DIR);

    {
        let mut config: Ini = match get_cgf(CONFIG_DIR) {
            Some(ini) => ini,
            None => {
                warn!("Ini not found. Creating new Ini");
                let _ = new_cfg(CONFIG_DIR);
                get_cgf(CONFIG_DIR).unwrap()
            }
        };

        game_dir = match attempt_locate_game(&mut config) {
            PathResult::Full(path) => {
                game_verified = true;
                path
            }
            PathResult::Partial(path) | PathResult::None(path) => {
                game_verified = false;
                path
            }
        };
        if !game_verified {
            ui.set_focus_page(1);
        } else {
            let display_mods = deserialize(&reg_mods);
            ui.global::<MainLogic>().set_current_mods(display_mods);
            ui.set_focus_page(0);
        };

        ui.global::<MainLogic>().set_game_path_valid(game_verified);
        ui.global::<SettingsLogic>()
            .set_game_path(game_dir.to_string_lossy().to_string().into());
    }

    // Use global rust mut variables for source of truth logic to ref current state | very slow to pull from UI global
    // Error check input text for invalid symbols | If mod_name already exists confirm overwrite dialog -> if array into entry -> remove_array fist
    // if selected file already exists as reg_mod -> error dialog | else success dialog mod_name with mod_files Registered
    ui.global::<MainLogic>().on_select_mod_files({
        let ui_handle = ui.as_weak();
        let game_dir_ref: Rc<PathBuf> = Rc::from(game_dir.clone());
        move || {
            // remember to handle unwrap errors like this one
            let mut config = get_cgf(CONFIG_DIR).unwrap();
            let ui = ui_handle.unwrap();
            let mod_name: String = ui.global::<MainLogic>().get_mod_name().to_string();
            if !game_verified {
                return;
            }
            let mod_files: Result<Vec<PathBuf>, &'static str> =
                match get_user_files(game_dir_ref.as_path()) {
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
                Ok(files) => match shorten_paths(files, &game_dir_ref) {
                    Ok(paths) => match paths.len() {
                        1 => {
                            save_path(
                                &mut config,
                                CONFIG_DIR,
                                Some("mod-files"),
                                &mod_name,
                                paths[0].as_path(),
                            );
                            reg_mods.push(RegMod {
                                name: mod_name.clone(),
                                state: true,
                                files: vec![paths[0].clone()],
                            })
                        }
                        _ => {
                            save_path_bufs(&mut config, CONFIG_DIR, &mod_name, &paths);
                            reg_mods.push(RegMod {
                                name: mod_name.clone(),
                                state: true,
                                files: paths.clone(),
                            })
                        }
                    },
                    Err(err) => {
                        error!("Error: {}", err);
                        return;
                    }
                },
                Err(err) => {
                    info!("{}", err);
                    return;
                }
            };
            save_bool(&mut config, CONFIG_DIR, &mod_name, true);
            ui.global::<MainLogic>()
                .set_mod_name(SharedString::from(""));
            let display_mods = deserialize(&reg_mods);
            ui.global::<MainLogic>().set_current_mods(display_mods);
        }
    });
    ui.global::<SettingsLogic>().on_select_game_dir({
        let ui_handle = ui.as_weak();
        let game_dir_ref: Rc<Path> = Rc::from(game_dir.clone().as_path());
        move || {
            // remember to handle unwrap errors like this one
            let mut config = get_cgf(CONFIG_DIR).unwrap();
            let ui = ui_handle.unwrap();
            let user_path: Result<String, &'static str> = match get_user_folder(&game_dir_ref) {
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
                            ui.global::<MainLogic>().set_game_path_valid(true);
                            ui.global::<SettingsLogic>()
                                .set_game_path(try_path.to_string_lossy().to_string().into());
                            save_path(
                                &mut config,
                                CONFIG_DIR,
                                Some("paths"),
                                "game_dir",
                                &try_path,
                            );
                            game_dir = try_path;
                            game_verified = true;
                        }
                        false => {
                            let msg: &str = "Failure: Files not found";
                            warn!("{}", &msg);
                            ui.set_err_message(SharedString::from(format!(
                                "Game files not found in: {}",
                                try_path.to_string_lossy()
                            )));
                            ui.invoke_show_error_popup();
                        }
                    }
                }
                Err(err) => {
                    info!("{}", err);
                    ui.set_err_message(SharedString::from(err));
                    ui.invoke_show_error_popup();
                }
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

pub fn deserialize(data: &[RegMod]) -> ModelRc<DisplayMod> {
    let display_mod: Rc<VecModel<DisplayMod>> = Default::default();
    for mod_data in data.iter() {
        display_mod.push(DisplayMod {
            name: SharedString::from(mod_data.name.clone()),
            enabled: mod_data.state,
            files: SharedString::from(
                mod_data
                    .files
                    .iter()
                    .map(|path_buf| path_buf.to_string_lossy().to_string())
                    .collect::<Vec<String>>()
                    .join("\r\n"),
            ),
        })
    }
    ModelRc::from(display_mod)
}

// Hides console on --release | cant read logs if console is hidden
//#![windows_subsystem = "windows"]

slint::include_modules!();

mod ini_tools {
    pub mod parser;
    pub mod writer;
}

use ini_tools::{parser::RegMod, writer::*};
use log::{debug, error, info, warn};
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
    // let mut game_dir: PathBuf;
    // let mut game_verified: bool;

    {
        match get_cfg(CONFIG_DIR) {
            Ok(_) => info!("Config file found at \"{}\"", &CONFIG_DIR),
            Err(err) => {
                error!("Error: {}", err);
                warn!("Ini not found. Creating new Ini");
                let _ = new_cfg(CONFIG_DIR);
                get_cfg(CONFIG_DIR).unwrap();
            }
        };

        let game_verified: bool;
        let game_dir = match attempt_locate_game(CONFIG_DIR) {
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
            ui.set_focus_page(0);
        };
        let reg_mods = RegMod::collect(CONFIG_DIR);
        ui.global::<MainLogic>()
            .set_current_mods(deserialize(&reg_mods));
        ui.global::<MainLogic>().set_game_path_valid(game_verified);
        ui.global::<SettingsLogic>()
            .set_game_path(game_dir.to_string_lossy().to_string().into());
    }

    // Use global rust mut variables for source of truth logic to ref current state | very slow to pull from UI global
    // Error check input text for invalid symbols | If mod_name already exists confirm overwrite dialog -> if array into entry -> remove_array fist
    // if selected file already exists as reg_mod -> error dialog | else success dialog mod_name with mod_files Registered
    // need fn for checking state of the files are all the same, if all files disabled need to save state as false
    ui.global::<MainLogic>().on_select_mod_files({
        let ui_handle = ui.as_weak();
        let game_verified = ui.global::<MainLogic>().get_game_path_valid();
        let game_dir = PathBuf::from(ui.global::<SettingsLogic>().get_game_path().to_string());
        let game_dir_ref: Rc<Path> = Rc::from(game_dir.as_path());
        move || {
            if !game_verified {
                return;
            }
            let ui = ui_handle.unwrap();
            let mod_name: String = ui.global::<MainLogic>().get_mod_name().to_string();
            let mod_files: Result<Vec<PathBuf>, &'static str> = match get_user_files(&game_dir_ref)
            {
                Ok(opt) => match opt.len() {
                    0 => Err("No files selected"),
                    _ => Ok(opt),
                },
                Err(err) => {
                    error!("{}", err);
                    Err("Error selecting path")
                }
            };
            // let local_game_dir = PathBuf::from(game_dir.to_string_lossy().to_string());
            match mod_files {
                Ok(files) => match shorten_paths(files, &game_dir) {
                    Ok(paths) => match paths.len() {
                        1 => {
                            save_path(CONFIG_DIR, Some("mod-files"), &mod_name, paths[0].as_path());
                        }
                        _ => {
                            save_path_bufs(CONFIG_DIR, &mod_name, &paths);
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
            save_bool(CONFIG_DIR, &mod_name, true);
            let reg_mods = RegMod::collect(CONFIG_DIR);
            ui.global::<MainLogic>()
                .set_mod_name(SharedString::from(""));
            ui.global::<MainLogic>()
                .set_current_mods(deserialize(&reg_mods));
        }
    });
    ui.global::<SettingsLogic>().on_select_game_dir({
        let ui_handle = ui.as_weak();
        let game_dir = PathBuf::from(ui.global::<SettingsLogic>().get_game_path().to_string());
        let game_dir_ref: Rc<Path> = Rc::from(game_dir.as_path());
        move || {
            // remember to handle unwrap errors like this one
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
                            save_path(CONFIG_DIR, Some("paths"), "game_dir", &try_path);
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
    ui.global::<MainLogic>().on_toggleMod({
        let game_dir = PathBuf::from(ui.global::<SettingsLogic>().get_game_path().to_string());
        move |key: SharedString| {
            let reg_mods = RegMod::collect(CONFIG_DIR);
            if let Some(found_mod) = reg_mods.iter().find(|reg_mod| key == reg_mod.name) {
                toggle_files(
                    &found_mod.name,
                    &game_dir,
                    !found_mod.state,
                    found_mod.files.clone(),
                    CONFIG_DIR,
                );
            } else {
                error!("Mod: \"{}\" not found", key);
            };
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

fn deserialize(data: &[RegMod]) -> ModelRc<DisplayMod> {
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

// Hides console on --release | cant read logs if console is hidden
#![windows_subsystem = "windows"]

slint::include_modules!();

use elden_mod_loader_gui::{
    ini_tools::{
        parser::{IniProperty, RegMod, Valitidity},
        writer::*,
    },
    *,
};
use i_slint_backend_winit::WinitWindowAccessor;
use log::{error, info, warn};
use native_dialog::FileDialog;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::{
    env,
    io::ErrorKind,
    path::{Path, PathBuf},
    rc::Rc,
};

#[macro_use]
extern crate lazy_static;

const CONFIG_NAME: &str = "mod_loader_config.ini";
lazy_static! {
    static ref CURRENT_INI: PathBuf = get_ini_dir();
}

fn main() -> Result<(), slint::PlatformError> {
    env_logger::init();
    slint::platform::set_platform(Box::new(i_slint_backend_winit::Backend::new().unwrap()))
        .expect("This app uses the winit backend");
    let ui = App::new()?;
    ui.window()
        .with_winit_window(|window: &winit::window::Window| {
            window.set_enabled_buttons(
                winit::window::WindowButtons::CLOSE | winit::window::WindowButtons::MINIMIZE,
            );
        });
    {
        let ini_valid = match get_cfg(&CURRENT_INI) {
            Ok(ini) => {
                if ini.is_setup() {
                    info!("Config file found at \"{}\"", &CURRENT_INI.display());
                    true
                } else {
                    false
                }
            }
            Err(err) => {
                error!("Error: {}", err);
                ui.display_err("Welcome to Elden Mod Loader GUI!\nThanks for downloading, please report any bugs");
                false
            }
        };
        if !ini_valid {
            warn!("Ini not setup correctly. Creating new Ini");
            new_cfg(&CURRENT_INI).unwrap();
        }

        let game_verified: bool;
        let game_dir: Option<PathBuf> = match attempt_locate_game(&CURRENT_INI) {
            Ok(path_result) => match path_result {
                PathResult::Full(path) => match RegMod::collect(&CURRENT_INI, false) {
                    Ok(reg_mods) => {
                        reg_mods.iter().for_each(|data| {
                            data.verify_state(&path, &CURRENT_INI)
                                .unwrap_or_else(|err| ui.display_err(&err.to_string()))
                        });
                        game_verified = true;
                        Some(path)
                    }
                    Err(err) => {
                        game_verified = false;
                        ui.display_err(&err.to_string());
                        None
                    }
                },
                PathResult::Partial(path) | PathResult::None(path) => {
                    game_verified = false;
                    Some(path)
                }
            },
            Err(err) => {
                ui.display_err(&err.to_string());
                game_verified = false;
                None
            }
        };

        match IniProperty::<bool>::read(
            &get_cfg(&CURRENT_INI).unwrap(),
            Some("app-settings"),
            "dark-mode",
            false,
        ) {
            Some(bool) => ui.global::<SettingsLogic>().set_dark_mode(bool.value),
            None => {
                ui.global::<SettingsLogic>().set_dark_mode(true);
                save_bool(&CURRENT_INI, Some("app-settings"), "dark-mode", true)
                    .unwrap_or_else(|err| ui.display_err(&err.to_string()));
            }
        };

        ui.global::<MainLogic>().set_game_path_valid(game_verified);
        ui.global::<SettingsLogic>().set_game_path(
            game_dir
                .unwrap_or(PathBuf::from(""))
                .to_string_lossy()
                .to_string()
                .into(),
        );
        if !game_verified {
            ui.global::<MainLogic>().set_current_subpage(1);
            ui.global::<MainLogic>().set_current_mods(deserialize(
                &RegMod::collect(&CURRENT_INI, false).unwrap_or_else(|err| {
                    ui.display_err(&err.to_string());
                    vec![RegMod::default()]
                }),
            ));
        } else {
            ui.global::<MainLogic>().set_current_mods(deserialize(
                &RegMod::collect(&CURRENT_INI, true).unwrap_or_else(|err| {
                    ui.display_err(&err.to_string());
                    vec![RegMod::default()]
                }),
            ));
        }
    }

    // Error check input text for invalid symbols | If mod_name already exists confirm overwrite dialog -> if array into entry -> remove_array fist
    // if selected file already exists as reg_mod -> error dialog | else success dialog mod_name with mod_files Registered
    // need fn for checking state of the files are all the same, if all files disabled need to save state as false
    // Error check for if selected file is already contained in a regested mod
    ui.global::<MainLogic>().on_select_mod_files({
        let ui_handle = ui.as_weak();
        let game_verified = ui.global::<MainLogic>().get_game_path_valid();
        let game_dir = PathBuf::from(ui.global::<SettingsLogic>().get_game_path().to_string());
        let game_dir_ref: Rc<Path> = Rc::from(game_dir.as_path());
        move |mod_name: SharedString| {
            if !game_verified {
                return;
            }
            let ui = ui_handle.unwrap();
            let mod_files = get_user_files(&game_dir_ref);
            match mod_files {
                Ok(files) => match shorten_paths(files, &game_dir) {
                    Ok(paths) => match paths.len() {
                        1 => {
                            save_path(
                                &CURRENT_INI,
                                Some("mod-files"),
                                &mod_name,
                                paths[0].as_path(),
                            )
                            .unwrap_or_else(|err| ui.display_err(&err.to_string()));
                        }
                        _ => {
                            save_path_bufs(&CURRENT_INI, &mod_name, &paths)
                                .unwrap_or_else(|err| ui.display_err(&err.to_string()));
                        }
                    },
                    Err(err) => {
                        error!("Error: {}", err);
                        ui.display_err(&err.to_string());
                        return;
                    }
                },
                Err(err) => {
                    info!("{}", err);
                    ui.display_err(err);
                    return;
                }
            };
            save_bool(&CURRENT_INI, Some("registered-mods"), &mod_name, true)
                .unwrap_or_else(|err| ui.display_err(&err.to_string()));
            // Add conditons here to keep line edit text the same
            ui.global::<MainLogic>()
                .set_line_edit_text(SharedString::from(""));
            ui.global::<MainLogic>().set_current_mods(deserialize(
                &RegMod::collect(&CURRENT_INI, false).unwrap_or_else(|err| {
                    ui.display_err(&err.to_string());
                    vec![RegMod::default()]
                }),
            ));
        }
    });
    ui.global::<SettingsLogic>().on_select_game_dir({
        let ui_handle = ui.as_weak();
        let game_dir = PathBuf::from(ui.global::<SettingsLogic>().get_game_path().to_string());
        let game_dir_ref: Rc<Path> = Rc::from(game_dir.as_path());

        move || {
            let ui = ui_handle.unwrap();
            let user_path: Result<String, &'static str> = match get_user_folder(&game_dir_ref) {
                Ok(opt) => match opt {
                    Some(selected_path) => Ok(selected_path.to_string_lossy().to_string()),
                    None => {
                        ui.display_err("No Path Selected");
                        Err("No Path Selected")
                    }
                },
                Err(err) => {
                    error!("Error selecting path");
                    error!("{}", err);
                    ui.display_err(&err.to_string());
                    Err("Error selecting path")
                }
            };

            match user_path {
                Ok(path) => {
                    info!("User Selected Path: \"{}\"", &path);
                    let try_path: PathBuf = if does_dir_contain(Path::new(&path), &["Game"]).is_ok()
                    {
                        PathBuf::from(&format!("{}\\Game", path))
                    } else {
                        PathBuf::from(&path)
                    };
                    match does_dir_contain(Path::new(&try_path), &REQUIRED_GAME_FILES) {
                        Ok(_) => {
                            info!("Success: Files found, saving diretory");
                            ui.global::<MainLogic>().set_game_path_valid(true);
                            ui.global::<SettingsLogic>()
                                .set_game_path(try_path.to_string_lossy().to_string().into());
                            save_path(&CURRENT_INI, Some("paths"), "game_dir", &try_path)
                                .unwrap_or_else(|err| ui.display_err(&err.to_string()));
                        }
                        Err(err) => {
                            match err.kind() {
                                ErrorKind::NotFound => warn!("{}", err),
                                _ => error!("Error: {}", err),
                            }
                            ui.display_err(&err.to_string())
                        }
                    }
                }
                Err(err) => {
                    info!("{}", err);
                    ui.display_err(err)
                }
            }
        }
    });
    ui.global::<MainLogic>().on_toggleMod({
        let ui_handle = ui.as_weak();
        let game_dir = PathBuf::from(ui.global::<SettingsLogic>().get_game_path().to_string());
        move |key: SharedString| {
            let ui = ui_handle.unwrap();
            match RegMod::collect(&CURRENT_INI, false) {
                Ok(reg_mods) => {
                    if let Some(found_mod) = reg_mods.iter().find(|reg_mod| key == reg_mod.name) {
                        toggle_files(
                            &found_mod.name,
                            &game_dir,
                            !found_mod.state,
                            found_mod.files.clone(),
                            &CURRENT_INI,
                        )
                        .unwrap_or_else(|err| ui.display_err(&err.to_string()));
                    } else {
                        error!("Mod: \"{}\" not found", key);
                    };
                }
                Err(err) => ui.display_err(&err.to_string()),
            }
        }
    });
    ui.global::<MainLogic>().on_force_app_focus({
        let ui_handle = ui.as_weak();
        move || {
            let ui = ui_handle.unwrap();
            ui.invoke_focus_app()
        }
    });
    ui.global::<MainLogic>().on_add_to_mod({
        let ui_handle = ui.as_weak();
        move |key: SharedString| {
            let ui = ui_handle.unwrap();
            let game_dir = PathBuf::from(ui.global::<SettingsLogic>().get_game_path().to_string());
            match get_user_files(&game_dir) {
                Ok(files) => {
                    match RegMod::collect(&CURRENT_INI, false) {
                        Ok(reg_mods) => {
                            if let Some(found_mod) =
                                reg_mods.iter().find(|reg_mod| key == reg_mod.name)
                            {
                                let mut new_data = found_mod.files.clone();
                                match shorten_paths(files, &game_dir) {
                                    Ok(short_paths) => {
                                        new_data.extend(short_paths.iter().cloned());
                                        if found_mod.files.len() == 1 {
                                            remove_entry(
                                                &CURRENT_INI,
                                                Some("mod-files"),
                                                &found_mod.name,
                                            )
                                            .unwrap_or_else(|err| ui.display_err(&err.to_string()));
                                        } else {
                                            remove_array(&CURRENT_INI, &found_mod.name)
                                                .unwrap_or_else(|err| {
                                                    ui.display_err(&err.to_string())
                                                });
                                        }
                                        save_path_bufs(&CURRENT_INI, &found_mod.name, &new_data)
                                            .unwrap_or_else(|err| ui.display_err(&err.to_string()));
                                        ui.global::<MainLogic>().set_current_mods(deserialize(
                                            &RegMod::collect(&CURRENT_INI, false).unwrap_or_else(
                                                |err| {
                                                    ui.display_err(&err.to_string());
                                                    vec![RegMod::default()]
                                                },
                                            ),
                                        ));
                                        // Make sure that user remains on correct page if mod order changes apon set_current_mods
                                    }
                                    Err(err) => {
                                        error!("{}", err);
                                        ui.display_err(&err.to_string());
                                    }
                                }
                            } else {
                                error!("Mod: \"{}\" not found", key);
                                ui.display_err(&format!("Mod: \"{}\" not found", key));
                            };
                        }
                        Err(err) => ui.display_err(&err.to_string()),
                    };
                }
                Err(err) => {
                    error!("{}", err);
                    ui.display_err(err);
                }
            }
        }
    });
    ui.global::<MainLogic>().on_remove_mod({
        let ui_handle = ui.as_weak();
        move |key: SharedString| {
            let ui = ui_handle.unwrap();
            match RegMod::collect(&CURRENT_INI, false) {
                Ok(reg_mods) => {
                    if let Some(found_mod) = reg_mods.iter().find(|reg_mod| key == reg_mod.name) {
                        if found_mod.files.iter().any(|file| {
                            file.extension().expect("file with extention") == "disabled"
                        }) {
                            let game_dir = PathBuf::from(
                                ui.global::<SettingsLogic>().get_game_path().to_string(),
                            );
                            toggle_files(
                                &found_mod.name,
                                &game_dir,
                                true,
                                found_mod.files.clone(),
                                &CURRENT_INI,
                            )
                            .unwrap_or_else(|err| ui.display_err(&err.to_string()));
                        }
                        remove_entry(&CURRENT_INI, Some("registered-mods"), &found_mod.name)
                            .unwrap_or_else(|err| ui.display_err(&err.to_string()));
                        // we can let sync keys take care of removing files from ini
                        ui.global::<MainLogic>().set_current_mods(deserialize(
                            &RegMod::collect(&CURRENT_INI, false).unwrap_or_else(|err| {
                                ui.display_err(&err.to_string());
                                vec![RegMod::default()]
                            }),
                        ));
                    } else {
                        error!("Mod: \"{}\" not found", key);
                    };
                }
                Err(err) => ui.display_err(&err.to_string()),
            };
            ui.global::<MainLogic>().set_current_subpage(0);
        }
    });
    ui.global::<SettingsLogic>().on_toggle_theme({
        let ui_handle = ui.as_weak();
        move |state| {
            let ui = ui_handle.unwrap();
            save_bool(&CURRENT_INI, Some("app-settings"), "dark-mode", state)
                .unwrap_or_else(|err| ui.display_err(&err.to_string()));
        }
    });

    ui.invoke_focus_app();
    ui.run()
}

impl App {
    fn display_err(&self, msg: &str) {
        self.set_err_message(SharedString::from(msg));
        self.invoke_show_error_popup();
    }
}

fn get_user_folder(path: &Path) -> Result<Option<PathBuf>, native_dialog::Error> {
    FileDialog::new().set_location(path).show_open_single_dir()
}

fn get_ini_dir() -> PathBuf {
    let exe_dir = env::current_dir().expect("Failed to get current dir");
    exe_dir.join(CONFIG_NAME)
}

fn get_user_files(path: &Path) -> Result<Vec<PathBuf>, &'static str> {
    match FileDialog::new()
        .set_location(path)
        .show_open_multiple_file()
    {
        Ok(opt) => match opt.len() {
            0 => Err("No files selected"),
            _ => Ok(opt),
        },
        Err(err) => {
            error!("{}", err);
            Err("Error selecting path")
        }
    }
}

fn deserialize(data: &[RegMod]) -> ModelRc<DisplayMod> {
    let display_mod: Rc<VecModel<DisplayMod>> = Default::default();
    for mod_data in data.iter() {
        display_mod.push(DisplayMod {
            displayname: SharedString::from(if mod_data.name.chars().count() > 20 {
                mod_data
                    .name
                    .clone()
                    .chars()
                    .enumerate()
                    .filter_map(|(i, c)| match i {
                        ..=17 => Some(c),
                        _ => None,
                    })
                    .chain("...".chars())
                    .collect()
            } else {
                mod_data.name.clone()
            }),
            name: SharedString::from(mod_data.name.clone()),
            enabled: mod_data.state,
            files: SharedString::from(
                mod_data
                    .files
                    .iter()
                    .map(|path_buf| {
                        path_buf
                            .to_string_lossy()
                            .to_string()
                            .replace(".disabled", "")
                    })
                    .collect::<Vec<String>>()
                    .join("\n"),
            ),
        })
    }
    ModelRc::from(display_mod)
}

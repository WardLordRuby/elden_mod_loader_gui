#![cfg(target_os = "windows")]
// Setting windows_subsystem will hide console | cant read logs if console is hidden
#![windows_subsystem = "windows"]

slint::include_modules!();

use elden_mod_loader_gui::{
    ini_tools::{
        parser::{split_out_config_files, IniProperty, RegMod, Valitidity},
        writer::*,
    },
    *,
};
use i_slint_backend_winit::WinitWindowAccessor;
use log::{error, info, warn};
use native_dialog::FileDialog;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::{
    ffi::OsString,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

const CONFIG_NAME: &str = "mod_loader_config.ini";
lazy_static::lazy_static! {
    static ref CURRENT_INI: PathBuf = get_ini_dir();
}

fn main() -> Result<(), slint::PlatformError> {
    env_logger::init();
    slint::platform::set_platform(Box::new(
        i_slint_backend_winit::Backend::new().expect("This app is being run on windows"),
    ))
    .expect("This app uses the winit backend");
    let ui = App::new()?;
    ui.window()
        .with_winit_window(|window: &winit::window::Window| {
            window.set_enabled_buttons(
                winit::window::WindowButtons::CLOSE | winit::window::WindowButtons::MINIMIZE,
            );
        });
    {
        let first_startup: bool;
        let ini_valid = match get_cfg(&CURRENT_INI) {
            Ok(ini) => {
                if ini.is_setup() {
                    info!("Config file found at \"{}\"", &CURRENT_INI.display());
                    first_startup = false;
                    true
                } else {
                    first_startup = false;
                    false
                }
            }
            Err(err) => {
                error!("Error: {err}");
                first_startup = true;
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
                                .unwrap_or_else(|err| ui.display_msg(&err.to_string()))
                        });
                        game_verified = true;
                        Some(path)
                    }
                    Err(err) => {
                        ui.display_msg(&err.to_string());
                        game_verified = true;
                        Some(path)
                    }
                },
                PathResult::Partial(path) | PathResult::None(path) => {
                    game_verified = false;
                    Some(path)
                }
            },
            Err(err) => {
                ui.display_msg(&err.to_string());
                game_verified = false;
                None
            }
        };

        match IniProperty::<bool>::read(
            &get_cfg(&CURRENT_INI).expect("ini file is verified"),
            Some("app-settings"),
            "dark-mode",
            false,
        ) {
            Some(bool) => ui.global::<SettingsLogic>().set_dark_mode(bool.value),
            None => {
                ui.global::<SettingsLogic>().set_dark_mode(true);
                save_bool(&CURRENT_INI, Some("app-settings"), "dark-mode", true)
                    .unwrap_or_else(|err| ui.display_msg(&err.to_string()));
            }
        };

        ui.global::<MainLogic>().set_game_path_valid(game_verified);
        ui.global::<SettingsLogic>().set_game_path(
            game_dir
                .clone()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
                .into(),
        );
        ui.global::<MainLogic>().set_current_mods(deserialize(
            &RegMod::collect(&CURRENT_INI, !game_verified).unwrap_or_else(|err| {
                ui.display_msg(&err.to_string());
                vec![RegMod::default()]
            }),
            &game_dir.unwrap_or_default().to_string_lossy(),
        ));
        if !game_verified {
            ui.global::<MainLogic>().set_current_subpage(1);
            if !first_startup {
                ui.display_msg(
                    "Failed to locate Elden Ring\nPlease Select the install directory for Elden Ring",
                );
            }
        }
        if first_startup && !game_verified {
            ui.display_msg(
                "Welcome to Elden Mod Loader GUI!\nThanks for downloading, please report any bugs\n\nPlease select the game directory containing \"eldenring.exe\"",
            );
        } else if first_startup && game_verified {
            ui.display_msg("Welcome to Elden Mod Loader GUI!\nThanks for downloading, please report any bugs\n\nGame Files Found!\nAdd mods to the app by entering a name and selecting mod files with \"Select Files\"\n\nYou can always add more files to a mod or de-register a mod at any time from within the app");
        }
    }

    // Error check input text for invalid symbols
    ui.global::<MainLogic>().on_select_mod_files({
        let ui_handle = ui.as_weak();
        move |mod_name: SharedString| {
            let ui = ui_handle.unwrap();
            let format_key = mod_name.trim().replace(' ', "_");
            let registered_mods = RegMod::collect(&CURRENT_INI, false).unwrap_or_else(|err| {
                ui.display_msg(&err.to_string());
                vec![RegMod::default()]
            });
            {
                if registered_mods
                    .iter()
                    .any(|mod_data| format_key.to_lowercase() == mod_data.name.to_lowercase())
                {
                    ui.display_msg(&format!(
                        "There is already a registered mod with the name\n\"{mod_name}\""
                    ));
                    ui.global::<MainLogic>()
                        .set_line_edit_text(SharedString::from(""));
                    return;
                }
            }
            let game_dir = PathBuf::from(ui.global::<SettingsLogic>().get_game_path().to_string());
            let game_dir_ref: Rc<Path> = Rc::from(game_dir.as_path());
            match get_user_files(&game_dir_ref) {
                Ok(file_paths) => match shorten_paths(file_paths, &game_dir) {
                    Ok(files) => {
                        if file_registered(&registered_mods, &files) {
                            ui.display_msg("A selected file is already registered to a mod");
                        } else {
                            let state = !files.iter().all(|file| {
                                file.extension().expect("file has extention") == "disabled"
                            });
                            save_bool(&CURRENT_INI, Some("registered-mods"), &format_key, state)
                                .unwrap_or_else(|err| ui.display_msg(&err.to_string()));
                            match files.len() {
                                0 => unreachable!(),
                                1 => {
                                    save_path(
                                        &CURRENT_INI,
                                        Some("mod-files"),
                                        &format_key,
                                        files[0].as_path(),
                                    )
                                    .unwrap_or_else(|err| ui.display_msg(&err.to_string()));
                                }
                                2.. => {
                                    save_path_bufs(&CURRENT_INI, &format_key, &files)
                                        .unwrap_or_else(|err| ui.display_msg(&err.to_string()));
                                }
                            }
                            let (config_files, files) = split_out_config_files(files);
                            RegMod {
                                name: format_key,
                                state,
                                files,
                                config_files,
                            }
                            .verify_state(&game_dir, &CURRENT_INI)
                            .unwrap_or_else(|err| ui.display_msg(&err.to_string()));
                            ui.global::<MainLogic>()
                                .set_line_edit_text(SharedString::from(""));
                            ui.global::<MainLogic>().set_current_mods(deserialize(
                                &RegMod::collect(&CURRENT_INI, false).unwrap_or_else(|err| {
                                    ui.display_msg(&err.to_string());
                                    vec![RegMod::default()]
                                }),
                                &game_dir.to_string_lossy(),
                            ));
                        }
                    }
                    Err(err) => {
                        error!("Error: {err}");
                        ui.display_msg("Mod files must be within the selected game directory");
                    }
                },
                Err(err) => {
                    info!("{err}");
                    ui.display_msg(err);
                }
            };
        }
    });
    ui.global::<SettingsLogic>().on_select_game_dir({
        let ui_handle = ui.as_weak();
        move || {
            let ui = ui_handle.unwrap();
            let game_dir = PathBuf::from(ui.global::<SettingsLogic>().get_game_path().to_string());
            let game_dir_ref = Rc::from(game_dir.as_path());
            let user_path = match get_user_folder(&game_dir_ref) {
                Ok(opt) => match opt {
                    Some(selected_path) => Ok(selected_path.to_string_lossy().to_string()),
                    None => Err("No Path Selected"),
                },
                Err(err) => {
                    error!("{err}");
                    Err("Error selecting path")
                }
            };

            match user_path {
                Ok(path) => {
                    info!("User Selected Path: \"{}\"", &path);
                    let try_path: PathBuf = if does_dir_contain(Path::new(&path), &["Game"]).is_ok()
                    {
                        PathBuf::from(&format!("{path}\\Game"))
                    } else {
                        PathBuf::from(path)
                    };
                    match does_dir_contain(Path::new(&try_path), &REQUIRED_GAME_FILES) {
                        Ok(_) => {
                            info!("Success: Files found, saving diretory");
                            ui.global::<MainLogic>().set_game_path_valid(true);
                            ui.global::<SettingsLogic>()
                                .set_game_path(try_path.to_string_lossy().to_string().into());
                            save_path(&CURRENT_INI, Some("paths"), "game_dir", &try_path)
                                .unwrap_or_else(|err| ui.display_msg(&err.to_string()));
                            ui.global::<MainLogic>().set_current_subpage(0);
                            ui.display_msg("Game Files Found!\nAdd mods to the app by entering a name and selecting mod files with \"Select Files\"\n\nYou can always add more files to a mod or de-register a mod at any time from within the app")
                        }
                        Err(err) => {
                            match err.kind() {
                                ErrorKind::NotFound => warn!("{err}"),
                                _ => error!("Error: {err}"),
                            }
                            ui.display_msg(&err.to_string())
                        }
                    }
                }
                Err(err) => {
                    info!("{err}");
                    ui.display_msg(err)
                }
            }
        }
    });
    ui.global::<MainLogic>().on_toggleMod({
        let ui_handle = ui.as_weak();
        move |key: SharedString| {
            let ui = ui_handle.unwrap();
            let game_dir = PathBuf::from(ui.global::<SettingsLogic>().get_game_path().to_string());
            let format_key = key.replace(' ', "_");
            match RegMod::collect(&CURRENT_INI, false) {
                Ok(reg_mods) => {
                    if let Some(found_mod) =
                        reg_mods.iter().find(|reg_mod| format_key == reg_mod.name)
                    {
                        toggle_files(&game_dir, !found_mod.state, found_mod, &CURRENT_INI)
                            .unwrap_or_else(|err| ui.display_msg(&err.to_string()));
                    } else {
                        error!("Mod: \"{key}\" not found");
                        ui.display_msg(&format!("Mod: \"{key}\" not found"))
                    };
                }
                Err(err) => ui.display_msg(&err.to_string()),
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
            let format_key = key.replace(' ', "_");
            let game_dir = PathBuf::from(ui.global::<SettingsLogic>().get_game_path().to_string());
            let registered_mods = match RegMod::collect(&CURRENT_INI, false) {
                Ok(data) => data,
                Err(err) => {
                    ui.display_msg(&err.to_string());
                    return;
                }
            };
            match get_user_files(&game_dir) {
                Ok(file_paths) => {
                    if let Some(found_mod) = registered_mods
                        .iter()
                        .find(|reg_mod| format_key == reg_mod.name)
                    {
                        match shorten_paths(file_paths, &game_dir) {
                            Ok(files) => {
                                if file_registered(&registered_mods, &files) {
                                    ui.display_msg(
                                        "A selected file is already registered to a mod",
                                    );
                                } else {
                                    let mut new_data = found_mod.files.clone();
                                    new_data.extend(files);
                                    if !found_mod.config_files.is_empty() {
                                        new_data.extend(found_mod.config_files.iter().cloned());
                                    }
                                    if found_mod.files.len() + found_mod.config_files.len() == 1 {
                                        remove_entry(
                                            &CURRENT_INI,
                                            Some("mod-files"),
                                            &found_mod.name,
                                        )
                                        .unwrap_or_else(|err| ui.display_msg(&err.to_string()));
                                    } else {
                                        remove_array(&CURRENT_INI, &found_mod.name)
                                            .unwrap_or_else(|err| ui.display_msg(&err.to_string()));
                                    }
                                    save_path_bufs(&CURRENT_INI, &found_mod.name, &new_data)
                                        .unwrap_or_else(|err| ui.display_msg(&err.to_string()));
                                    let (config_files, files) =
                                        split_out_config_files(new_data.clone());
                                    RegMod {
                                        name: found_mod.name.clone(),
                                        state: found_mod.state,
                                        files,
                                        config_files,
                                    }
                                    .verify_state(&game_dir, &CURRENT_INI)
                                    .unwrap_or_else(|err| ui.display_msg(&err.to_string()));
                                    ui.global::<MainLogic>().set_current_mods(deserialize(
                                        &RegMod::collect(&CURRENT_INI, false).unwrap_or_else(
                                            |err| {
                                                ui.display_msg(&err.to_string());
                                                vec![RegMod::default()]
                                            },
                                        ),
                                        &game_dir.to_string_lossy(),
                                    ));
                                }
                            }
                            Err(err) => {
                                error!("{err}");
                                ui.display_msg(
                                    "Mod files must be within the selected game directory",
                                );
                            }
                        }
                    } else {
                        error!("Mod: \"{key}\" not found");
                        ui.display_msg(&format!("Mod: \"{key}\" not found"));
                    };
                }
                Err(err) => {
                    error!("{err}");
                    ui.display_msg(err);
                }
            }
        }
    });
    ui.global::<MainLogic>().on_remove_mod({
        let ui_handle = ui.as_weak();
        move |key: SharedString| {
            let ui = ui_handle.unwrap();
            let format_key = key.replace(' ', "_");
            match RegMod::collect(&CURRENT_INI, false) {
                Ok(reg_mods) => {
                    let game_dir =
                        PathBuf::from(ui.global::<SettingsLogic>().get_game_path().to_string());
                    if let Some(found_mod) =
                        reg_mods.iter().find(|reg_mod| format_key == reg_mod.name)
                    {
                        if found_mod.files.iter().any(|file| {
                            file.extension().expect("file with extention") == "disabled"
                        }) {
                            toggle_files(&game_dir, true, found_mod, &CURRENT_INI)
                                .unwrap_or_else(|err| ui.display_msg(&err.to_string()));
                        }
                        remove_entry(&CURRENT_INI, Some("registered-mods"), &found_mod.name)
                            .unwrap_or_else(|err| ui.display_msg(&err.to_string()));
                        // we can let sync keys take care of removing files from ini
                        ui.global::<MainLogic>().set_current_mods(deserialize(
                            &RegMod::collect(&CURRENT_INI, false).unwrap_or_else(|err| {
                                ui.display_msg(&err.to_string());
                                vec![RegMod::default()]
                            }),
                            &game_dir.to_string_lossy(),
                        ));
                    } else {
                        error!("Mod: \"{key}\" not found");
                    };
                }
                Err(err) => ui.display_msg(&err.to_string()),
            };
            ui.global::<MainLogic>().set_current_subpage(0);
        }
    });
    ui.global::<SettingsLogic>().on_toggle_theme({
        let ui_handle = ui.as_weak();
        move |state| {
            let ui = ui_handle.unwrap();
            save_bool(&CURRENT_INI, Some("app-settings"), "dark-mode", state)
                .unwrap_or_else(|err| ui.display_msg(&err.to_string()));
        }
    });
    ui.global::<MainLogic>().on_edit_config({
        let ui_handle = ui.as_weak();
        move |config_file| {
            let ui = ui_handle.unwrap();
            let downcast_config_file = config_file
                .as_any()
                .downcast_ref::<VecModel<SharedString>>()
                .expect("We know we set a VecModel earlier");
            let string_file = downcast_config_file
                .iter()
                .map(|path| OsString::from(path.to_string()))
                .collect::<Vec<_>>();
            for file in string_file {
                let arc_file = Arc::new(file);
                let clone_file = arc_file.clone();
                let jh = std::thread::spawn(move || {
                    std::process::Command::new("notepad")
                        .arg(&*arc_file)
                        .spawn()
                });
                if let Err(err) = jh
                    .join()
                    .unwrap_or(Err(io::Error::new(io::ErrorKind::Other, "Thread panicked")))
                {
                    match err.kind() {
                        io::ErrorKind::Other => {
                            error!("{err}");
                            ui.display_msg("notepad exited unexpectedly")
                        }
                        _ => {
                            error!("Could not open Notepad. Error: {err}");
                            ui.display_msg(&format!(
                                "Error: Failed to open mod config file {clone_file:?}"
                            ));
                        }
                    }
                }
            }
        }
    });

    ui.invoke_focus_app();
    ui.run()
}

impl App {
    fn display_msg(&self, msg: &str) {
        self.set_err_message(SharedString::from(msg));
        self.invoke_show_error_popup();
    }
}

fn get_user_folder(path: &Path) -> Result<Option<PathBuf>, native_dialog::Error> {
    FileDialog::new().set_location(path).show_open_single_dir()
}

fn get_ini_dir() -> PathBuf {
    let exe_dir = std::env::current_dir().expect("Failed to get current dir");
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
            error!("{err}");
            Err("Error selecting path")
        }
    }
}

fn file_registered(mod_data: &[RegMod], files: &[PathBuf]) -> bool {
    files.iter().any(|path| {
        mod_data
            .iter()
            .any(|registered_mod| registered_mod.files.iter().any(|mod_file| path == mod_file))
    })
}

fn deserialize(data: &[RegMod], game_dir: &str) -> ModelRc<DisplayMod> {
    let display_mod: Rc<VecModel<DisplayMod>> = Default::default();
    for mod_data in data.iter() {
        let has_config = !mod_data.config_files.is_empty();
        let config_files: Rc<VecModel<SharedString>> = Default::default();
        if has_config {
            mod_data.config_files.iter().for_each(|file| {
                config_files.push(SharedString::from(format!(
                    "{game_dir}\\{}",
                    file.display()
                )))
            })
        } else {
            config_files.push(SharedString::new())
        };
        let name = mod_data.name.replace('_', " ");
        display_mod.push(DisplayMod {
            displayname: SharedString::from(if mod_data.name.chars().count() > 20 {
                format!("{}...", &name[..17])
            } else {
                name.clone()
            }),
            name: SharedString::from(name.clone()),
            enabled: mod_data.state,
            files: SharedString::from({
                let files = mod_data
                    .files
                    .iter()
                    .map(|path_buf| path_buf.to_string_lossy().replace(".disabled", ""))
                    .collect::<Vec<_>>()
                    .join("\n");
                let config_files = mod_data
                    .config_files
                    .iter()
                    .map(|f| f.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("{files}\n{config_files}")
            }),
            has_config,
            config_files: ModelRc::from(config_files),
        })
    }
    ModelRc::from(display_mod)
}

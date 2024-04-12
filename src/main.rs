#![cfg(target_os = "windows")]
// Setting windows_subsystem will hide console | cant read logs if console is hidden
// #![windows_subsystem = "windows"]

use elden_mod_loader_gui::{
    utils::{
        ini::{
            parser::{file_registered, IniProperty, RegMod, Valitidity},
            writer::*,
        },
        installer::{remove_mod_files, InstallData, scan_for_mods}
    },
    *,
};
use i_slint_backend_winit::WinitWindowAccessor;
use log::{error, info, warn};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::{
    ffi::OsStr, io::ErrorKind, path::{Path, PathBuf}, rc::Rc, sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    }
};
use tokio::sync::{
    mpsc::{unbounded_channel, UnboundedReceiver},
    Mutex,
};

slint::include_modules!();

const CONFIG_NAME: &str = "EML_gui_config.ini";
static GLOBAL_NUM_KEY: AtomicU32 = AtomicU32::new(0);
lazy_static::lazy_static! {
    static ref CURRENT_INI: PathBuf = get_ini_dir();
    static ref RESTRICTED_FILES: [&'static OsStr; 6] = populate_restricted_files();
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
        }
    );
    let (message_sender, message_receiver) = unbounded_channel::<MessageData>();
    let receiver = Arc::new(Mutex::new(message_receiver));
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
        let game_dir = match attempt_locate_game(&CURRENT_INI) {
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
            "dark_mode",
            false,
        ) {
            Some(bool) => ui.global::<SettingsLogic>().set_dark_mode(bool.value),
            None => {
                ui.global::<SettingsLogic>().set_dark_mode(true);
                save_bool(&CURRENT_INI, Some("app-settings"), "dark_mode", true)
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
        ));
        let mod_loader: ModLoader;
        if !game_verified {
            ui.global::<MainLogic>().set_current_subpage(1);
            if !first_startup {
                ui.display_msg(
                    "Failed to locate Elden Ring\nPlease Select the install directory for Elden Ring",
                );
            }
            mod_loader = ModLoader::default();
        } else {
            let game_dir = game_dir.clone().expect("game dir verified");
            mod_loader = elden_mod_loader_properties(&game_dir).unwrap_or_default();
            ui.global::<SettingsLogic>()
                .set_loader_disabled(mod_loader.disabled);
            if mod_loader.installed {
                ui.global::<SettingsLogic>().set_loader_installed(true);
                if let Ok(mod_loader_ini) = get_cfg(&mod_loader.cfg) {
                    match IniProperty::<u32>::read(
                        &mod_loader_ini,
                        LOADER_SECTIONS[0],
                        LOADER_KEYS[0],
                        false,
                    ) {
                        Some(delay_time) => ui
                            .global::<SettingsLogic>()
                            .set_load_delay(SharedString::from(format!("{}ms", delay_time.value))),
                        None => {
                            error!("Found an unexpected character saved in \"load_delay\" Reseting to default value");
                            save_value_ext(
                                &mod_loader.cfg,
                                LOADER_SECTIONS[0],
                                LOADER_KEYS[0],
                                "5000",
                            )
                            .unwrap_or_else(|err| error!("{err}"));
                        }
                    }
                } else {
                    error!("Error: could not read \"mod_loader_config.ini\"");
                }
            }
            if !first_startup && !mod_loader.installed {
                ui.display_msg("This tool requires Elden Mod Loader by TechieW to be installed!");
            }
        }
        if first_startup {
            if !game_verified && !mod_loader.installed {
                ui.display_msg(
                    "Welcome to Elden Mod Loader GUI!\nThanks for downloading, please report any bugs\n\nCould not find Elden Mod Loader Script!\nThis tool requires Elden Mod Loader by TechieW to be installed!\n\nPlease select the game directory containing \"eldenring.exe\"",
                );
            } else if game_verified && !mod_loader.installed {
                ui.display_msg("Welcome to Elden Mod Loader GUI!\nThanks for downloading, please report any bugs\n\nGame Files Found!\n\nCould not find Elden Mod Loader Script!\nThis tool requires Elden Mod Loader by TechieW to be installed!\n\nAdd mods to the app by entering a name and selecting mod files with \"Select Files\"\n\nYou can always add more files to a mod or de-register a mod at any time from within the app");
            } else if game_verified {
                let receiver_clone = receiver.clone();
                let ui_handle = ui.as_weak();
                slint::spawn_local(async move {
                    let ui = ui_handle.unwrap();
                    let ui_handle = ui.as_weak();
                    match confirm_scan_mods(ui_handle, receiver_clone.clone(), &game_dir.expect("game verified"), &CURRENT_INI, false).await {
                        Ok(len) => {
                            ui.global::<MainLogic>().set_current_mods(deserialize(
                                &RegMod::collect(&CURRENT_INI, false).unwrap_or_else(|err| {
                                    ui.display_msg(&err.to_string());
                                    vec![RegMod::default()]
                                }),
                            ));
                            ui.display_msg(&format!("Successfully Found {len} mod(s)"));
                            let _ = receive_msg(receiver_clone).await;
                        }
                        Err(err) => if err.kind() != ErrorKind::ConnectionAborted {
                            ui.display_msg(&format!("Error: {err}"));
                            let _ = receive_msg(receiver_clone).await;
                        }
                    };
                    ui.display_msg("Welcome to Elden Mod Loader GUI!\nThanks for downloading, please report any bugs\n\nGame Files Found!\nAdd mods to the app by entering a name and selecting mod files with \"Select Files\"\n\nYou can always add more files to a mod or de-register a mod at any time from within the app\n\nDo not forget to disable easy anti-cheat before playing with mods installed!");
                }).unwrap();
            }
        }
    }

    // TODO: Error check input text for invalid symbols
    ui.global::<MainLogic>().on_select_mod_files({
        let ui_handle = ui.as_weak();
        let receiver_clone = receiver.clone();
        move |mod_name| {
            let ui = ui_handle.unwrap();
            let format_key = mod_name.trim().replace(' ', "_");
            let mut results: Vec<std::io::Result<()>> = Vec::with_capacity(2);
            let registered_mods = RegMod::collect(&CURRENT_INI, false).unwrap_or_else(|err| {
                results.push(Err(err));
                vec![RegMod::default()]
            });
            if !results.is_empty() {
                ui.display_msg(&results[0].as_ref().unwrap_err().to_string());
                return;
            }
            {
                if registered_mods
                    .iter()
                    .any(|mod_data| format_key.to_lowercase() == mod_data.name.to_lowercase())
                {
                    ui.display_msg(&format!(
                        "There is already a registered mod with the name\n\"{mod_name}\""
                    ));
                    ui.global::<MainLogic>()
                        .set_line_edit_text(SharedString::new());
                    return;
                }
            }
            let game_dir = PathBuf::from(ui.global::<SettingsLogic>().get_game_path().to_string());
            let receiver = receiver_clone.clone();
            slint::spawn_local(async move {
                let file_paths = match get_user_files(&game_dir) {
                    Ok(files) => files,
                    Err(err) => {
                        info!("{err}");
                        ui.display_msg(&err.to_string());
                        return;
                    }
                };
                let files = match shorten_paths(&file_paths, &game_dir) {
                    Ok(files) => files,
                    Err(err) => {
                        if file_paths.len() == err.err_paths_long.len() {
                            let ui_handle = ui.as_weak();
                            match install_new_mod(&mod_name, err.err_paths_long, &game_dir, ui_handle, receiver).await {
                                Ok(installed_files) => {
                                    match shorten_paths(&installed_files, &game_dir) {
                                        Ok(installed_and_shortend) => installed_and_shortend,
                                        Err(err) => {
                                            let err_string = format!("New mod installed but ran into StripPrefixError on {:?}", err.err_paths_long);
                                            error!("{err_string}");
                                            ui.display_msg(&err_string);
                                            return;
                                        }
                                    }
                                },
                                Err(err) => {
                                    match err.kind() {
                                        ErrorKind::ConnectionAborted => info!("{err}"),
                                        _ => error!("{err}"),
                                    }
                                    ui.display_msg(&err.to_string());
                                    return;
                                }
                            }
                        } else {
                            error!("Encountered {} StripPrefixError on input files", err.err_paths_long.len());
                            ui.display_msg(&format!("Some selected files are already installed\n\nSelected Files Installed: {}\nSelected Files not installed: {}", err.ok_paths_short.len(), err.err_paths_long.len()));
                            return;
                        }
                    }
                };
                if file_registered(&registered_mods, &files) {
                    ui.display_msg("A selected file is already registered to a mod");
                } else {
                    let state = !files.iter().all(|file| {
                        file.extension().expect("file has extention") == "disabled"
                    });
                    results.push(save_bool(
                        &CURRENT_INI,
                        Some("registered-mods"),
                        &format_key,
                        state,
                    ));
                    match files.len() {
                        0 => return,
                        1 => results.push(save_path(
                            &CURRENT_INI,
                            Some("mod-files"),
                            &format_key,
                            files[0].as_path(),
                        )),
                        2.. => {
                            let path_refs = files.iter().map(|p| p.as_path()).collect::<Vec<_>>();
                            results.push(save_path_bufs(&CURRENT_INI, &format_key, &path_refs))
                        },
                    }
                    if let Some(err) = results.iter().find_map(|result| result.as_ref().err()) {
                        ui.display_msg(&err.to_string());
                        // If something fails to save attempt to create a corrupt entry so
                        // sync keys will take care of any invalid ini entries
                        let _ =
                        remove_entry(&CURRENT_INI, Some("registered-mods"), &format_key);
                    }
                    let new_mod = RegMod::new(&format_key, state, files);
                    
                    new_mod
                    .verify_state(&game_dir, &CURRENT_INI)
                    .unwrap_or_else(|err| {
                        // Toggle files returned an error lets try it again
                        if new_mod.verify_state(&game_dir, &CURRENT_INI).is_err() {
                            ui.display_msg(&err.to_string());
                            let _ = remove_entry(
                                &CURRENT_INI,
                                Some("registered-mods"),
                                &new_mod.name,
                            );
                        };
                    });
                    ui.global::<MainLogic>()
                    .set_line_edit_text(SharedString::new());
                    ui.global::<MainLogic>().set_current_mods(deserialize(
                        &RegMod::collect(&CURRENT_INI, false).unwrap_or_else(|_| {
                            // if error lets try it again and see if we can get sync-keys to cleanup any errors
                            match RegMod::collect(&CURRENT_INI, false) {
                                Ok(mods) => mods,
                                Err(err) => {
                                    ui.display_msg(&err.to_string());
                                    vec![RegMod::default()]
                                }
                            }
                        }),
                    ));
                }
            }).unwrap();
        }
    });
    ui.global::<SettingsLogic>().on_select_game_dir({
        let ui_handle = ui.as_weak();
        move || {
            let ui = ui_handle.unwrap();
            let game_dir = PathBuf::from(ui.global::<SettingsLogic>().get_game_path().to_string());
            slint::spawn_local(async move {
                let path_result = get_user_folder(&game_dir);
                let path = match path_result {
                    Ok(path) => path,
                    Err(err) => {
                        info!("{err}");
                        ui.display_msg(&err.to_string());
                        return;
                    }
                };
                info!("User Selected Path: \"{}\"", path.display());
                let try_path: PathBuf = match does_dir_contain(&path, Operation::All, &["Game"])
                {
                    Ok(true) => PathBuf::from(&format!("{}\\Game", path.display())),
                    Ok(false) => path, 
                    Err(err) => {
                        error!("{err}");
                        ui.display_msg(&err.to_string());
                        return;
                    }
                };
                match does_dir_contain(Path::new(&try_path), Operation::All, &REQUIRED_GAME_FILES) {
                    Ok(true) => {
                        let result = save_path(&CURRENT_INI, Some("paths"), "game_dir", &try_path);
                        if result.is_err() && save_path(&CURRENT_INI, Some("paths"), "game_dir", &try_path).is_err() {
                            let err = result.unwrap_err();
                            error!("Failed to save directory. {err}");
                            ui.display_msg(&err.to_string());
                            return;
                        };
                        info!("Success: Files found, saved diretory");
                        let mod_loader = match elden_mod_loader_properties(&try_path) {
                            Ok(loader) => loader,
                            Err(err) => {
                                error!("{err}");
                                ui.display_msg(&err.to_string());
                                return;
                            }
                        };
                        ui.global::<SettingsLogic>()
                            .set_game_path(try_path.to_string_lossy().to_string().into());
                        ui.global::<MainLogic>().set_game_path_valid(true);
                        ui.global::<MainLogic>().set_current_subpage(0);
                        ui.global::<SettingsLogic>().set_loader_installed(mod_loader.installed);
                        ui.global::<SettingsLogic>().set_loader_disabled(mod_loader.disabled);
                        if mod_loader.installed {
                            ui.display_msg("Game Files Found!\nAdd mods to the app by entering a name and selecting mod files with \"Select Files\"\n\nYou can always add more files to a mod or de-register a mod at any time from within the app\n\nDo not forget to disable easy anti-cheat before playing with mods installed!")
                        } else {
                            ui.display_msg("Game Files Found!\n\nCould not find Elden Mod Loader Script!\nThis tool requires Elden Mod Loader by TechieW to be installed!")
                        }
                    }
                    Ok(false) => {
                        let err = format!("Required Game files not found in:\n\"{}\"", try_path.display());
                        error!("{err}");
                        ui.display_msg(&err);
                    }
                    Err(err) => {
                        match err.kind() {
                            ErrorKind::NotFound => warn!("{err}"),
                            _ => error!("Error: {err}"),
                        }
                        ui.display_msg(&err.to_string())
                    }
                }
            }).unwrap();
        }
    });
    ui.global::<MainLogic>().on_toggleMod({
        let ui_handle = ui.as_weak();
        move |key, state| {
            let ui = ui_handle.unwrap();
            let game_dir = PathBuf::from(ui.global::<SettingsLogic>().get_game_path().to_string());
            let format_key = key.replace(' ', "_");
            match RegMod::collect(&CURRENT_INI, false) {
                Ok(reg_mods) => {
                    if let Some(found_mod) =
                        reg_mods.iter().find(|reg_mod| format_key == reg_mod.name)
                    {
                        let result = toggle_files(&game_dir, state, found_mod, Some(&CURRENT_INI));
                        if result.is_ok() {
                            return;
                        }
                        let err = result.unwrap_err();
                        ui.display_msg(&err.to_string());
                    } else {
                        error!("Mod: \"{key}\" not found");
                        ui.display_msg(&format!("Mod: \"{key}\" not found"))
                    };
                }
                Err(err) => ui.display_msg(&err.to_string()),
            }
            ui.global::<MainLogic>().set_if_err_bool(!state);
            ui.global::<MainLogic>().set_current_mods(deserialize(
                &RegMod::collect(&CURRENT_INI, false).unwrap_or_else(|_| {
                    // if error lets try it again and see if we can get sync-keys to cleanup any errors
                    match RegMod::collect(&CURRENT_INI, false) {
                        Ok(mods) => mods,
                        Err(err) => {
                            ui.display_msg(&err.to_string());
                            vec![RegMod::default()]
                        }
                    }
                }),
            ));
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
        let reciever_clone = receiver.clone();
        move |key| {
            let ui = ui_handle.unwrap();
            let format_key = key.replace(' ', "_");
            let game_dir = PathBuf::from(
                ui.global::<SettingsLogic>().get_game_path().to_string(),
            );
            let registered_mods = match RegMod::collect(&CURRENT_INI, false) {
                Ok(data) => data,
                Err(err) => {
                    ui.display_msg(&err.to_string());
                    return;
                }
            };
            let reciever_clone = reciever_clone.clone();
            slint::spawn_local(async move {
                let file_paths = match get_user_files(&game_dir) {
                    Ok(paths) => paths,
                    Err(err) => {
                        error!("{err}");
                        ui.display_msg(&err.to_string());
                        return;
                    }
                };
                if let Some(found_mod) = registered_mods
                    .iter()
                    .find(|reg_mod| format_key == reg_mod.name)
                {
                    let files = match shorten_paths(&file_paths, &game_dir) {
                        Ok(files) => files,
                        Err(err) => {
                            if file_paths.len() == err.err_paths_long.len() {
                                let ui_handle = ui.as_weak();
                                match install_new_files_to_mod(found_mod, err.err_paths_long, &game_dir, ui_handle, reciever_clone).await {
                                    Ok(installed_files) => {
                                        match shorten_paths(&installed_files, &game_dir) {
                                            Ok(installed_and_shortend) => installed_and_shortend,
                                            Err(err) => {
                                                let err_string = format!("Files installed but ran into StripPrefixError on {:?}", err.err_paths_long);
                                                error!("{err_string}");
                                                ui.display_msg(&err_string);
                                                return;
                                            }
                                        }
                                    },
                                    Err(err) => {
                                        match err.kind() {
                                            ErrorKind::ConnectionAborted => info!("{err}"),
                                            _ => error!("{err}"),
                                        }
                                        ui.display_msg(&err.to_string());
                                        return;
                                    }
                                }
                            } else {
                                error!("Encountered {} StripPrefixError on input files", err.err_paths_long.len());
                                ui.display_msg(&format!("Some selected files are already installed\n\nSelected Files Installed: {}\nSelected Files not installed: {}", err.ok_paths_short.len(), err.err_paths_long.len()));
                                return;
                            }
                        }
                    };
                    if file_registered(&registered_mods, &files) {
                        ui.display_msg("A selected file is already registered to a mod");
                    } else {
                        let mut new_data = found_mod.files.clone();
                        new_data.extend(files);
                        let mut results = Vec::with_capacity(2);
                        let new_data_refs = found_mod.add_other_files_to_files(&new_data);
                        if found_mod.files.len() + found_mod.other_files_len() == 1 {
                            results.push(remove_entry(
                                &CURRENT_INI,
                                Some("mod-files"),
                                &found_mod.name,
                            ));
                        } else {
                            results.push(remove_array(&CURRENT_INI, &found_mod.name));
                        }
                        results.push(save_path_bufs(&CURRENT_INI, &found_mod.name, &new_data_refs));
                        if let Some(err) = results.iter().find_map(|result| result.as_ref().err()) {
                            ui.display_msg(&err.to_string());
                            let _ = remove_entry(
                                &CURRENT_INI,
                                Some("registered-mods"),
                                &format_key,
                            );
                        }
                        let new_data_owned = new_data_refs.iter().map(PathBuf::from).collect();
                        let updated_mod = RegMod::new(&found_mod.name, found_mod.state, new_data_owned);
                        
                        updated_mod
                            .verify_state(&game_dir, &CURRENT_INI)
                            .unwrap_or_else(|err| {
                                if updated_mod
                                    .verify_state(&game_dir, &CURRENT_INI)
                                    .is_err()
                                {
                                    ui.display_msg(&err.to_string());
                                    let _ = remove_entry(
                                        &CURRENT_INI,
                                        Some("registered-mods"),
                                        &updated_mod.name,
                                    );
                                };
                            });
                        ui.global::<MainLogic>().set_current_mods(deserialize(
                            &RegMod::collect(&CURRENT_INI, false).unwrap_or_else(|_| {
                                match RegMod::collect(&CURRENT_INI, false) {
                                    Ok(mods) => mods,
                                    Err(err) => {
                                        ui.display_msg(&err.to_string());
                                        vec![RegMod::default()]
                                    }
                                }
                            }),
                        ));
                    }
                } else {
                    error!("Mod: \"{key}\" not found");
                    ui.display_msg(&format!("Mod: \"{key}\" not found"));
                };
            })
            .unwrap();
        }
    });
    ui.global::<MainLogic>().on_remove_mod({
        let ui_handle = ui.as_weak();
        let receiver_clone = receiver.clone();
        move |key| {
            let ui = ui_handle.unwrap();
            let format_key = key.replace(' ', "_");
            ui.display_confirm(&format!("Are you sure you want to de-register: \"{key}\""), false);
            let receiver = receiver_clone.clone();
            slint::spawn_local(async move {
                if receive_msg(receiver.clone()).await != Message::Confirm {
                    return
                }
                let reg_mods = match RegMod::collect(&CURRENT_INI, false) {
                    Ok(reg_mods) => reg_mods,
                    Err(err) => {
                        ui.display_msg(&err.to_string());
                        return;
                    }
                        
                };
                let game_dir =
                    PathBuf::from(ui.global::<SettingsLogic>().get_game_path().to_string());
                if let Some(found_mod) =
                    reg_mods.iter().find(|reg_mod| format_key == reg_mod.name)
                {
                    let mut found_files = found_mod.files.clone();
                    if found_files.iter().any(|file| {
                        file.extension().expect("file with extention") == "disabled"
                    }) {
                        match toggle_files(&game_dir, true, found_mod, Some(&CURRENT_INI)) {
                            Ok(files) => found_files = files,
                            Err(err) => {
                                ui.display_msg(&format!("Failed to set mod to enabled state on removal\naborted before removal\n\n{err}"));
                                return;
                            }
                        }
                    }
                    // we can let sync keys take care of removing files from ini
                    remove_entry(&CURRENT_INI, Some("registered-mods"), &found_mod.name)
                        .unwrap_or_else(|err| ui.display_msg(&err.to_string()));
                    let file_refs = found_mod.add_other_files_to_files(&found_files);
                    let ui_handle = ui.as_weak();
                    match confirm_remove_mod(ui_handle, receiver, &game_dir, file_refs).await {
                        Ok(_) => ui.display_msg(&format!("Successfully removed all files associated with the previously registered mod \"{key}\"")),
                        Err(err) => {
                            match err.kind() {
                                ErrorKind::ConnectionAborted => info!("{err}"),
                                _ => error!("{err}"),
                            }
                            ui.display_msg(&err.to_string())
                        }
                    }
                } else {
                    let err = &format!("Mod: \"{key}\" not found");
                    error!("{err}");
                    ui.display_msg(&format!("{err}\nRemoving invalid entries"))
                };
                ui.global::<MainLogic>().set_current_subpage(0);
                ui.global::<MainLogic>().set_current_mods(deserialize(
                    &RegMod::collect(&CURRENT_INI, false).unwrap_or_else(|_| {
                        match RegMod::collect(&CURRENT_INI, false) {
                            Ok(mods) => mods,
                            Err(err) => {
                                ui.display_msg(&err.to_string());
                                vec![RegMod::default()]
                            }
                        }
                    }),
                ));
            }).unwrap();
        }
    });
    ui.global::<SettingsLogic>().on_toggle_theme({
        let ui_handle = ui.as_weak();
        move |state| {
            let ui = ui_handle.unwrap();
            save_bool(&CURRENT_INI, Some("app-settings"), "dark_mode", state).unwrap_or_else(
                |err| ui.display_msg(&format!("Failed to save theme preference\n\n{err}")),
            );
        }
    });
    ui.global::<MainLogic>().on_edit_config({
        let ui_handle = ui.as_weak();
        move |config_file| {
            let ui = ui_handle.unwrap();
            let game_dir = ui.global::<SettingsLogic>().get_game_path();
            let downcast_config_file = config_file
                .as_any()
                .downcast_ref::<VecModel<SharedString>>()
                .expect("We know we set a VecModel earlier");
            let string_file = downcast_config_file
                .iter()
                .map(|path| std::ffi::OsString::from(format!("{game_dir}\\{path}")))
                .collect::<Vec<_>>();
            for file in string_file {
                let arc_file = Arc::new(file);
                let clone_file = arc_file.clone();
                let jh = std::thread::spawn(move || {
                    std::process::Command::new("notepad")
                        .arg(&*arc_file)
                        .spawn()
                });
                match jh.join() {
                    Ok(result) => match result {
                        Ok(_) => (),
                        Err(err) => {
                            error!("{err}");
                            ui.display_msg(&format!(
                                "Failed to open config file {clone_file:?}\n\nError: {err}"
                            ));
                        }
                    },
                    Err(err) => {
                        error!("Thread panicked! {err:?}");
                        ui.display_msg(&format!("{err:?}"));
                    }
                }
            }
        }
    });
    ui.global::<SettingsLogic>().on_toggle_terminal({
        let ui_handle = ui.as_weak();
        move |state| {
            let ui = ui_handle.unwrap();
            let value = if state { "1" } else { "0" };
            let ext_ini = PathBuf::from(ui.global::<SettingsLogic>().get_game_path().to_string())
                .join(LOADER_FILES[0]);
            save_value_ext(&ext_ini, LOADER_SECTIONS[0], LOADER_KEYS[1], value).unwrap_or_else(
                |err| {
                    ui.display_msg(&err.to_string());
                    ui.global::<SettingsLogic>().set_show_terminal(!state);
                },
            );
        }
    });
    ui.global::<SettingsLogic>().on_set_load_delay({
        let ui_handle = ui.as_weak();
        move |time| {
            let ui = ui_handle.unwrap();
            let ext_ini = PathBuf::from(ui.global::<SettingsLogic>().get_game_path().to_string())
                .join(LOADER_FILES[0]);
            ui.global::<MainLogic>().invoke_force_app_focus();
            if let Err(err) = save_value_ext(&ext_ini, LOADER_SECTIONS[0], LOADER_KEYS[0], &time) {
                ui.display_msg(&format!("Failed to set load delay\n\n{err}"));
                return;
            }
            ui.global::<SettingsLogic>()
                .set_load_delay(SharedString::from(format!("{time}ms")));
            ui.global::<SettingsLogic>()
                .set_delay_input(SharedString::new());
        }
    });
    ui.global::<SettingsLogic>().on_toggle_all({
        let ui_handle = ui.as_weak();
        move |state| {
            let ui = ui_handle.unwrap();
            let game_dir = PathBuf::from(ui.global::<SettingsLogic>().get_game_path().to_string());
            let files = if state {
                vec![PathBuf::from(LOADER_FILES[1])]
            } else {
                vec![PathBuf::from(LOADER_FILES_DISABLED[1])]
            };
            let main_dll = RegMod::new("main", !state, files);
            match toggle_files(&game_dir, !state, &main_dll, None) {
                Ok(_) => ui.global::<SettingsLogic>().set_loader_disabled(state),
                Err(err) => {
                    ui.display_msg(&format!("{err}"));
                    ui.global::<SettingsLogic>().set_loader_disabled(!state)
                }
            }
        }
    });
    ui.global::<SettingsLogic>().on_open_game_dir({
        let ui_handle = ui.as_weak();
        move || {
            let ui = ui_handle.unwrap();
            let game_dir = ui.global::<SettingsLogic>().get_game_path().to_string();
            let jh = std::thread::spawn(move || {
                std::process::Command::new("explorer").arg(game_dir).spawn()
            });
            match jh.join() {
                Ok(result) => match result {
                    Ok(_) => (),
                    Err(err) => {
                        error!("{err}");
                        ui.display_msg(&format!("Error: {err}"));
                    }
                },
                Err(err) => {
                    error!("Thread panicked! {err:?}");
                    ui.display_msg(&format!("{err:?}"));
                }
            }
        }
    });
    ui.global::<MainLogic>().on_send_message({
        move |message| {
            let sender_clone = message_sender.clone();
            let key = GLOBAL_NUM_KEY.load(Ordering::Acquire);
            sender_clone
                .send(MessageData { message, key })
                .unwrap_or_else(|err| error!("{err}"));
        }
    });
    ui.global::<SettingsLogic>().on_scan_for_mods({
        let ui_handle = ui.as_weak();
        let receiver_clone = receiver.clone();
        move || {
            let ui = ui_handle.unwrap();
            let receiver_clone = receiver_clone.clone();
            slint::spawn_local(async move {
                let ui_handle = ui.as_weak();
                let game_dir = PathBuf::from(ui.global::<SettingsLogic>().get_game_path().to_string());
                match confirm_scan_mods(ui_handle, receiver_clone, &game_dir, &CURRENT_INI, true).await {
                    Ok(len) => {
                        ui.global::<MainLogic>().set_current_subpage(0);
                        ui.global::<MainLogic>().set_current_mods(deserialize(
                            &RegMod::collect(&CURRENT_INI, false).unwrap_or_else(|err| {
                                ui.display_msg(&err.to_string());
                                vec![RegMod::default()]
                            }),
                        ));
                        ui.display_msg(&format!("Successfully Found {len} mod(s)"));
                    }
                    Err(err) => if err.kind() != ErrorKind::ConnectionAborted {
                        ui.display_msg(&format!("Error: {err}"));
                    }
                };
            }).unwrap();
        }
    });

    ui.invoke_focus_app();
    ui.run()
}

impl App {
    pub fn display_msg(&self, msg: &str) {
        self.set_display_message(SharedString::from(msg));
        self.invoke_show_error_popup();
    }

    pub fn display_confirm(&self, msg: &str, alt_buttons: bool) {
        self.set_alt_std_buttons(alt_buttons);
        self.set_display_message(SharedString::from(msg));
        self.invoke_show_confirm_popup();
    }
}

struct MessageData {
    message: Message,
    key: u32,
}

async fn receive_msg(receiver: Arc<Mutex<UnboundedReceiver<MessageData>>>) -> Message {
    let key = GLOBAL_NUM_KEY.fetch_add(1, Ordering::SeqCst) + 1;
    let mut message = Message::Esc;
    let mut guard = receiver.lock().await;
    while let Some(msg) = guard.recv().await {
        if msg.key == key {
            message = msg.message;
            break;
        }
    }
    message
}

// Slint snapshot 1.6.0 offers a way to access WindowHandle for setting parent with rfd api
fn get_user_folder(path: &Path) -> Result<PathBuf, std::io::Error> {
    match rfd::FileDialog::new().set_directory(path).pick_folder() {
        Some(file) => Ok(file),
        None => new_io_error!(ErrorKind::InvalidInput, "No Path Selected"),
    }
}

fn get_user_files(path: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    match rfd::FileDialog::new().set_directory(path).pick_files() {
        Some(files) => match files.len() {
            0 => new_io_error!(ErrorKind::InvalidInput, "No Files Selected"),
            _ => {
                if files.iter().any(|file| {
                    RESTRICTED_FILES.iter().any(|&restricted_file| {
                        file.file_name().expect("has valid name") == restricted_file
                    })
                }) {
                    return new_io_error!(
                        ErrorKind::InvalidData,
                        "Error: Tried to add a restricted file"
                    );
                }
                Ok(files)
            }
        },
        None => {
            new_io_error!(ErrorKind::InvalidInput, "No Files Selected")
        }
    }
}

fn get_ini_dir() -> PathBuf {
    let exe_dir = std::env::current_dir().expect("Failed to get current dir");
    exe_dir.join(CONFIG_NAME)
}

fn populate_restricted_files() -> [&'static OsStr; 6] {
    let mut restricted_files: [&OsStr; 6] = [&OsStr::new(""); 6];
    for (i, file) in LOADER_FILES.iter().map(OsStr::new).enumerate() {
        restricted_files[i] = file;
    }
    for (i, file) in REQUIRED_GAME_FILES.iter().map(OsStr::new).enumerate() {
        restricted_files[i + LOADER_FILES.len()] = file;
    }
    restricted_files[LOADER_FILES.len() + REQUIRED_GAME_FILES.len()] =
        OsStr::new(LOADER_FILES_DISABLED[1]);

    restricted_files
}

fn deserialize(data: &[RegMod]) -> ModelRc<DisplayMod> {
    let display_mod: Rc<VecModel<DisplayMod>> = Default::default();
    for mod_data in data.iter() {
        let has_config = !mod_data.config_files.is_empty();
        let config_files: Rc<VecModel<SharedString>> = Default::default();
        if has_config {
            mod_data.config_files.iter().for_each(|file| {
                config_files.push(SharedString::from(file.to_string_lossy().to_string()))
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
                let files: Vec<String> = {
                    let mut files = Vec::with_capacity(mod_data.files.len() + mod_data.config_files.len() + mod_data.other_files.len());
                    files.extend(mod_data.files.iter().map(|f| f.to_string_lossy().replace(".disabled", "")));
                    files.extend(mod_data.config_files.iter().map(|f| f.to_string_lossy().to_string()));
                    files.extend(mod_data.other_files.iter().map(|f| f.to_string_lossy().to_string()));
                    files
                };
                files.join("\n")
            }),
            has_config,
            config_files: ModelRc::from(config_files),
        })
    }
    ModelRc::from(display_mod)
}

async fn install_new_mod(
    name: &str,
    files: Vec<PathBuf>,
    game_dir: &Path,
    ui_weak: slint::Weak<App>,
    receiver: Arc<Mutex<UnboundedReceiver<MessageData>>>,
) -> std::io::Result<Vec<PathBuf>> {
    let ui = ui_weak.unwrap();
    let mod_name = name.trim();
    ui.display_confirm(
        &format!(
            "Mod files are not installed in game directory.\nAttempt to install \"{mod_name}\"?"
        ),
        true,
    );
    if receive_msg(receiver.clone()).await != Message::Confirm {
        return new_io_error!(ErrorKind::ConnectionAborted, "Mod install canceled");
    }
    match InstallData::new(mod_name, files, game_dir) {
        Ok(data) => add_dir_to_install_data(data, ui_weak, receiver).await,
        Err(err) => Err(err)
    }
}

async fn install_new_files_to_mod(
    mod_data: &RegMod,
    files: Vec<PathBuf>,
    game_dir: &Path,
    ui_weak: slint::Weak<App>,
    receiver: Arc<Mutex<UnboundedReceiver<MessageData>>>,
) -> std::io::Result<Vec<PathBuf>> {
    let ui = ui_weak.unwrap();
    ui.display_confirm("Selected files are not installed? Would you like to try and install them?", true);
    if receive_msg(receiver.clone()).await != Message::Confirm {
        return new_io_error!(ErrorKind::ConnectionAborted, "Did not select to install files");
    };
    match InstallData::amend(mod_data, files, game_dir) {
        Ok(data) => confirm_install(data, ui_weak, receiver).await,
        Err(err) => Err(err)
    }
}

async fn add_dir_to_install_data(
    mut install_files: InstallData,
    ui_weak: slint::Weak<App>,
    receiver: Arc<Mutex<UnboundedReceiver<MessageData>>>,
) -> std::io::Result<Vec<PathBuf>> {
    let ui = ui_weak.unwrap();
    ui.display_confirm(&format!(
        "Current Files to install:\n{}\n\nWould you like to add a directory eg. Folder containing a config file?", 
        install_files.display_paths), true);
    let mut result: Vec<Result<(), std::io::Error>> = Vec::with_capacity(2);
    match receive_msg(receiver.clone()).await {
        Message::Confirm => match get_user_folder(&install_files.parent_dir) {
            Ok(path) => {
                install_files
                    .update_fields_with_new_dir(&path, utils::installer::DisplayItems::Limit(9))
                    .await
                    .unwrap_or_else(|err| {
                        error!("{err}");
                        result.push(Err(err));
                    });
            }
            Err(err) => result.push(Err(err)),
        },
        Message::Deny => (),
        Message::Esc => return new_io_error!(ErrorKind::ConnectionAborted, "Mod install canceled"),
    }
    match result.is_empty() {
        false => {
            let err = result[0].as_ref().unwrap_err();
            if result.len() == 1 && err.kind() == ErrorKind::InvalidInput {
                ui.display_msg(&format!("Error:\n\n{err}"));
                let _ = receive_msg(receiver.clone()).await;
                let future = async {
                    add_dir_to_install_data(install_files, ui_weak, receiver).await
                };
                let reselect_dir = Box::pin(future);
                reselect_dir.await
            } else {
                new_io_error!(ErrorKind::Other, format!("Error: Could not Install\n\n{err}"))
            }
        }
        true => confirm_install(install_files, ui_weak, receiver).await,
    }
}

async fn confirm_install(
    install_files: InstallData,
    ui_weak: slint::Weak<App>,
    receiver: Arc<Mutex<UnboundedReceiver<MessageData>>>,
) -> std::io::Result<Vec<PathBuf>> {
    let ui = ui_weak.unwrap();
    ui.display_confirm(
        &format!(
            "Confirm install of mod \"{}\"\n\nSelected files:\n{}\n\nInstall at:\n{}",
            install_files.name,
            install_files.display_paths,
            &install_files.install_dir.display()
        ),
        false,
    );
    if receive_msg(receiver).await != Message::Confirm {
        return new_io_error!(ErrorKind::ConnectionAborted, "Mod install canceled");
    }
    let zip = install_files.zip_from_to_paths()?;
    if zip.iter().any(|(_, to_path)| !matches!(to_path.try_exists(), Ok(false))) {
        return new_io_error!(ErrorKind::InvalidInput, format!("Could not install \"{}\".\nA selected file is already installed", install_files.name));
    };
    let parents = zip.iter().map(|(_, to_path)| parent_or_err(to_path)).collect::<std::io::Result<Vec<&Path>>>()?;
    parents.iter().try_for_each(std::fs::create_dir_all)?;
    zip.iter().try_for_each(|(from_path, to_path)| std::fs::copy(from_path, to_path).map(|_| ()))?;
    ui.display_msg(&format!("Successfully Installed mod \"{}\"", &install_files.name));
    Ok(zip.iter().map(|(_, to_path)| to_path.to_path_buf()).collect::<Vec<_>>())
}

async fn confirm_remove_mod(
    ui_weak: slint::Weak<App>,
    receiver: Arc<Mutex<UnboundedReceiver<MessageData>>>,
    game_dir: &Path, files: Vec<&Path>) -> std::io::Result<()> {
    let ui = ui_weak.unwrap();
    let install_dir = match files.iter().min_by_key(|file| file.ancestors().count()) {
        Some(path) => game_dir.join(parent_or_err(path)?),
        None => PathBuf::from("Error: Failed to display a parent_dir"),
    };
    ui.display_confirm("Do you want to remove mod files from the game directory?", true);
    if receive_msg(receiver.clone()).await != Message::Confirm {
        return new_io_error!(ErrorKind::ConnectionAborted, format!("Mod files are still installed at \"{}\"", install_dir.display()));
    };
    ui.display_confirm("This is a distructive action. Are you sure you want to continue?", false);
    if receive_msg(receiver).await != Message::Confirm {
        return new_io_error!(ErrorKind::ConnectionAborted, format!("Mod files are still installed at \"{}\"", install_dir.display()));
    };
    remove_mod_files(game_dir, files)
}

async fn confirm_scan_mods(
    ui_weak: slint::Weak<App>,
    receiver: Arc<Mutex<UnboundedReceiver<MessageData>>>,
    game_dir: &Path,
    ini_file: &Path,
    ini_exists: bool) -> std::io::Result<usize> {
    let ui = ui_weak.unwrap();
    ui.display_confirm("Would you like to attempt to auto-import already installed mods to Elden Mod Loader GUI?", true);
    if receive_msg(receiver.clone()).await != Message::Confirm {
        return new_io_error!(ErrorKind::ConnectionAborted, "Did not select to scan for mods");
    };
    if ini_exists {
        ui.display_confirm("Warning: This action will reset current registered mods, are you sure you want to continue?", true);
        if receive_msg(receiver.clone()).await != Message::Confirm {
            return new_io_error!(ErrorKind::ConnectionAborted, "Did not select to scan for mods");
        };
        let dark_mode = ui.global::<SettingsLogic>().get_dark_mode();
        std::fs::remove_file(ini_file)?;
        new_cfg(ini_file)?;
        save_bool(ini_file, Some("app-settings"), "dark_mode", dark_mode)?;
        save_path(ini_file, Some("paths"), "game_dir", game_dir)?;
    }
    scan_for_mods(game_dir, ini_file) 
}
#![cfg(target_os = "windows")]
// Setting windows_subsystem will hide console | cant read logs if console is hidden
// #![windows_subsystem = "windows"]

use elden_mod_loader_gui::{
    utils::{
        ini::{
            mod_loader::{Countable, ModLoader, ModLoaderCfg},
            parser::{file_registered, RegMod, Setup},
            writer::*,
        },
        installer::{remove_mod_files, scan_for_mods, InstallData}
    },
    *,
};
use i_slint_backend_winit::WinitWindowAccessor;
use log::{error, info, warn};
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, VecModel};
use winit::raw_window_handle::HasWindowHandle;
use std::{
    collections::{HashMap, HashSet}, ffi::OsStr, io::ErrorKind, path::{Path, PathBuf}, rc::Rc, sync::{
        atomic::{AtomicU32, Ordering},
        OnceLock,
    }
};
use tokio::sync::{
    mpsc::{unbounded_channel, UnboundedReceiver},
    RwLock,
};

slint::include_modules!();

static GLOBAL_NUM_KEY: AtomicU32 = AtomicU32::new(0);
static RESTRICTED_FILES: OnceLock<[&'static OsStr; 6]> = OnceLock::new();
static RECEIVER: OnceLock<RwLock<UnboundedReceiver<MessageData>>> = OnceLock::new();

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
    RECEIVER.set(RwLock::new(message_receiver)).unwrap();
    RESTRICTED_FILES.set(populate_restricted_files()).unwrap();
    {
        let current_ini = get_ini_dir();
        let mut errors= Vec::new();
        let first_startup: bool;
        let ini = match current_ini.is_setup(&INI_SECTIONS) {
            Ok(ini_data) => {
                first_startup = false;
                Some(ini_data)
            }
            Err(err) => {
                // MARK: TODO
                // create paths for these different error cases
                first_startup = matches!(err.kind(), ErrorKind::NotFound | ErrorKind::PermissionDenied | ErrorKind::InvalidData);
                error!("error 1: {err}");
                if !first_startup || err.kind() == ErrorKind::InvalidData { errors.push(err) }
                None
            }
        };
        let mut ini = match ini {
            Some(ini_data) => Cfg::from(ini_data, current_ini),
            None => {
                Cfg::read(current_ini).unwrap_or_else(|err| {
                    // io::write error
                    error!("error 2: {err}");
                    errors.push(err);
                    Cfg::default(current_ini)
                })
            }
        };

        let game_verified: bool;
        let mod_loader: ModLoader;
        let mut mod_loader_cfg: ModLoaderCfg;
        let mut reg_mods = None;
        let mut order_data: HashMap<String, usize>;
        let game_dir = match ini.attempt_locate_game() {
            Ok(path_result) => match path_result {
                PathResult::Full(path) => {
                    mod_loader = ModLoader::properties(&path).unwrap_or_else(|err| {
                        error!("error 3: {err}");
                        errors.push(err);
                        ModLoader::default()
                    });
                    if mod_loader.installed() {
                        mod_loader_cfg = ModLoaderCfg::read_section(mod_loader.path(), LOADER_SECTIONS[1]).unwrap_or_else(|err| {
                            error!("error 4: {err}");
                            errors.push(err);
                            ModLoaderCfg::default(mod_loader.path())
                        });
                    } else {
                        mod_loader_cfg = ModLoaderCfg::default(mod_loader.path());
                    }
                    order_data = match mod_loader_cfg.parse_section() {
                        Ok(data) => data,
                        Err(err) => {
                            error!("error 5: {err}");
                            errors.push(err);
                            HashMap::new()
                        }
                    };
                    match ini.collect_mods(Some(&order_data), false) {
                        Ok(mod_data) => reg_mods = Some(mod_data),
                        Err(err) => {
                            // io::Write error | PermissionDenied
                            error!("error 6: {err}");
                            errors.push(err);
                        }
                    };
                    if let Some(ref mods) = reg_mods {
                        if let Err(err) = mod_loader_cfg.verify_keys(mods) {
                            if err.kind() == ErrorKind::Unsupported {
                                order_data = mod_loader_cfg.parse_into_map();
                                match ini.collect_mods(Some(&order_data), false) {
                                    Ok(mod_data) => reg_mods = Some(mod_data),
                                    Err(err) => {
                                        // io::Write error | PermissionDenied
                                        error!("error 7: {err}");
                                        errors.push(err);
                                    }
                                };                                
                            }
                            errors.push(err);
                        }
                    }
                    if reg_mods.is_some() && reg_mods.as_ref().unwrap().len() != ini.mods_registered() {
                        ini = Cfg::read(current_ini).unwrap_or_else(|err| {
                            error!("error 8: {err}");
                            errors.push(err);
                            Cfg::default(current_ini)
                        })
                    }
                    game_verified = true;
                    Some(path)
                },
                PathResult::Partial(path) | PathResult::None(path) => {
                    mod_loader_cfg = ModLoaderCfg::empty();
                    mod_loader = ModLoader::default();
                    order_data = HashMap::new();
                    game_verified = false;
                    Some(path)
                }
            },
            Err(err) => {
                // io::Write error
                error!("error 9: {err}");
                errors.push(err);
                mod_loader_cfg = ModLoaderCfg::empty();
                mod_loader = ModLoader::default();
                order_data = HashMap::new();
                game_verified = false;
                None
            }
        };

        ui.global::<SettingsLogic>().set_dark_mode(ini.get_dark_mode().unwrap_or_else(|err| {
            // parse error ErrorKind::InvalidData
            error!("error 10: {err}");
            errors.push(err);
            DEFAULT_INI_VALUES[0].parse().unwrap()
        }));

        ui.global::<MainLogic>().set_game_path_valid(game_verified);
        ui.global::<SettingsLogic>().set_game_path(
            game_dir
                .as_ref()
                .unwrap_or(&PathBuf::new())
                .to_string_lossy()
                .to_string()
                .into(),
        );
        let _ = get_or_update_game_dir(Some(game_dir.as_ref().unwrap_or(&PathBuf::new()).to_owned()));

        if !game_verified {
            ui.global::<MainLogic>().set_current_subpage(1);
        } else {
            deserialize_current_mods(
                &if let Some(mod_data) = reg_mods {
                    mod_data 
                } else { 
                    ini.collect_mods(Some(&order_data),!mod_loader.installed()).unwrap_or_else(|err| {
                        // io::Error from toggle files | ErrorKind::InvalidInput - did not pass len check | io::Write error
                        error!("error 11: {err}");
                        errors.push(err);
                        vec![RegMod::default()]
                    })
                },ui.as_weak()
            );
            ui.global::<SettingsLogic>().set_loader_disabled(mod_loader.disabled());

            if mod_loader.installed() {
                ui.global::<SettingsLogic>().set_loader_installed(true);
                let delay = mod_loader_cfg.get_load_delay().unwrap_or_else(|err| {
                    // parse error ErrorKind::InvalidData
                    error!("error 12: {err}");
                    errors.push(err);
                    DEFAULT_LOADER_VALUES[0].parse().unwrap()
                });
                let show_terminal = mod_loader_cfg.get_show_terminal().unwrap_or_else(|err| {
                    // parse error ErrorKind::InvalidData
                    error!("error 13: {err}");
                    errors.push(err);
                    false
                });

                ui.global::<SettingsLogic>().set_load_delay(SharedString::from(format!("{}ms", delay)));
                ui.global::<SettingsLogic>().set_show_terminal(show_terminal);
            }
        }
        // we need to wait for slint event loop to start `ui.run()` before making calls to `ui.display_msg()`
        // otherwise calculations for the positon of display_msg_popup are not correct
        let ui_handle = ui.as_weak();
        slint::invoke_from_event_loop(move || {
            Timer::single_shot(std::time::Duration::from_millis(200), move || {
                slint::spawn_local(async move {
                    let ui = ui_handle.unwrap();
                    if !errors.is_empty() {
                        for err in errors {
                            ui.display_msg(&err.to_string());
                            let _ = receive_msg().await;
                        }
                    }
                    if first_startup {
                        if !game_verified {
                            ui.display_msg(
                                "Welcome to Elden Mod Loader GUI!\nThanks for downloading, please report any bugs\n\nPlease select the game directory containing \"eldenring.exe\"",
                            );
                        } else if game_verified && !mod_loader.installed() {
                            ui.display_msg("Welcome to Elden Mod Loader GUI!\nThanks for downloading, please report any bugs\n\nGame Files Found!\n\nCould not find Elden Mod Loader Script!\nThis tool requires Elden Mod Loader by TechieW to be installed!\n\nAdd mods to the app by entering a name and selecting mod files with \"Select Files\"\n\nYou can always add more files to a mod or de-register a mod at any time from within the app");
                        } else if game_verified {
                            ui.display_msg("Welcome to Elden Mod Loader GUI!\nThanks for downloading, please report any bugs\n\nGame Files Found!\nAdd mods to the app by entering a name and selecting mod files with \"Select Files\"\n\nYou can always add more files to a mod or de-register a mod at any time from within the app\n\nDo not forget to disable easy anti-cheat before playing with mods installed!");
                            let _ = receive_msg().await;
                            if let Err(err) = confirm_scan_mods(ui.as_weak(), &game_dir.expect("game_verified"), Some(ini)).await {
                                ui.display_msg(&err.to_string());
                            };
                        }
                    } else if game_verified {
                        if !mod_loader.installed() {
                            ui.display_msg(&format!("This tool requires Elden Mod Loader by TechieW to be installed!\n\nPlease install files to \"{}\"\nand relaunch Elden Mod Loader GUI", get_or_update_game_dir(None).display()));
                        } else if ini.mods_empty() {
                            if let Err(err) = confirm_scan_mods(ui.as_weak(), &game_dir.expect("game_verified"), Some(ini)).await {
                                ui.display_msg(&err.to_string());
                            }
                        }
                    } else {
                        ui.display_msg(
                            "Failed to locate Elden Ring\nPlease Select the install directory for Elden Ring",
                        );
                    }
                }).unwrap();
            });
        }).unwrap();
    }

    // TODO: Error check input text for invalid symbols
    ui.global::<MainLogic>().on_select_mod_files({
        let ui_handle = ui.as_weak();
        move |mod_name| {
            let ui = ui_handle.unwrap();
            let ini_dir = get_ini_dir();
            let mut ini = match Cfg::read(ini_dir) {
                Ok(ini_data) => ini_data,
                Err(err) => {
                    ui.display_msg(&err.to_string());
                    return;
                }
            };
            let format_key = mod_name.trim().replace(' ', "_");
            let mut results: Vec<std::io::Result<()>> = Vec::with_capacity(2);
            let registered_mods = ini.collect_mods(None, false).unwrap_or_else(|err| {
                results.push(Err(err));
                vec![RegMod::default()]
            });
            if !results.is_empty() {
                ui.display_msg(&results[0].as_ref().unwrap_err().to_string());
                return;
            }
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
            slint::spawn_local(async move {
                let game_dir = get_or_update_game_dir(None);
                let file_paths = match get_user_files(&game_dir, ui.as_weak()) {
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
                            match install_new_mod(&mod_name, err.err_paths_long, &game_dir, ui_handle).await {
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
                    return;
                }
                let state = !files.iter().all(FileData::is_disabled);
                results.push(save_bool(
                    ini.path(),
                    INI_SECTIONS[2],
                    &format_key,
                    state,
                ));
                match files.len() {
                    0 => return,
                    1 => results.push(save_path(
                        ini.path(),
                        INI_SECTIONS[3],
                        &format_key,
                        files[0].as_path(),
                    )),
                    2.. => {
                        let path_refs = files.iter().map(|p| p.as_path()).collect::<Vec<_>>();
                        results.push(save_paths(ini.path(), INI_SECTIONS[3], &format_key, &path_refs))
                    },
                }
                if let Some(err) = results.iter().find_map(|result| result.as_ref().err()) {
                    ui.display_msg(&err.to_string());
                    // If something fails to save attempt to create a corrupt entry so
                    // sync keys will take care of any invalid ini entries
                    let _ =
                    remove_entry(ini.path(), INI_SECTIONS[2], &format_key);
                }
                let new_mod = RegMod::new(&format_key, state, files);
                
                new_mod
                .verify_state(&game_dir, ini.path())
                .unwrap_or_else(|err| {
                    // Toggle files returned an error lets try it again
                    if new_mod.verify_state(&game_dir, ini.path()).is_err() {
                        ui.display_msg(&err.to_string());
                        let _ = remove_entry(
                            ini.path(),
                            INI_SECTIONS[2],
                            &new_mod.name,
                        );
                    };
                });
                ui.global::<MainLogic>().set_line_edit_text(SharedString::new());
                ini.update().unwrap_or_else(|err| {
                    ui.display_msg(&err.to_string());
                    ini = Cfg::default(ini_dir);
                });
                let order_data = order_data_or_default(ui.as_weak(), None);
                deserialize_current_mods(
                    &ini.collect_mods(Some(&order_data), false).unwrap_or_else(|_| {
                        // if error lets try it again and see if we can get sync-keys to cleanup any errors
                        ini.collect_mods( Some(&order_data), false).unwrap_or_else(|err| {
                            ui.display_msg(&err.to_string());
                                vec![RegMod::default()]
                        })
                    }),ui.as_weak()
                );
            }).unwrap();
        }
    });
    ui.global::<SettingsLogic>().on_select_game_dir({
        let ui_handle = ui.as_weak();
        move || {
            let ui = ui_handle.unwrap();
            let ini = match Cfg::read(get_ini_dir()) {
                Ok(ini_data) => ini_data,
                Err(err) => {
                    ui.display_msg(&err.to_string());
                    return;
                }
            };
            slint::spawn_local(async move {
                let game_dir = get_or_update_game_dir(None);
                let path_result = get_user_folder(&game_dir, ui.as_weak());
                drop(game_dir);
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
                    Ok(OperationResult::Bool(true)) => PathBuf::from(&format!("{}\\Game", path.display())),
                    Ok(OperationResult::Bool(false)) => path, 
                    Err(err) => {
                        error!("{err}");
                        ui.display_msg(&err.to_string());
                        return;
                    }
                    _ => unreachable!(),
                };
                match files_not_found(&try_path, &REQUIRED_GAME_FILES) {
                    Ok(not_found) => if not_found.is_empty() {
                        let result = save_path(ini.path(), INI_SECTIONS[1], INI_KEYS[1], &try_path);
                        if result.is_err() && save_path(ini.path(), INI_SECTIONS[1], INI_KEYS[1], &try_path).is_err() {
                            let err = result.unwrap_err();
                            error!("Failed to save directory. {err}");
                            ui.display_msg(&err.to_string());
                            return;
                        };
                        info!("Success: Files found, saved diretory");
                        let mod_loader = ModLoader::properties(&try_path).unwrap_or_default();
                        ui.global::<SettingsLogic>()
                            .set_game_path(try_path.to_string_lossy().to_string().into());
                        ui.global::<MainLogic>().set_game_path_valid(true);
                        ui.global::<MainLogic>().set_current_subpage(0);
                        ui.global::<SettingsLogic>().set_loader_installed(mod_loader.installed());
                        ui.global::<SettingsLogic>().set_loader_disabled(mod_loader.disabled());
                        if mod_loader.installed() {
                            ui.display_msg("Game Files Found!\nAdd mods to the app by entering a name and selecting mod files with \"Select Files\"\n\nYou can always add more files to a mod or de-register a mod at any time from within the app\n\nDo not forget to disable easy anti-cheat before playing with mods installed!");
                            let _ = receive_msg().await;
                            if ini.mods_empty() {
                                if let Err(err) = confirm_scan_mods(ui.as_weak(), &try_path, Some(ini)).await {
                                    ui.display_msg(&err.to_string());
                                };
                            }
                        } else {
                            ui.display_msg("Game Files Found!\n\nCould not find Elden Mod Loader Script!\nThis tool requires Elden Mod Loader by TechieW to be installed!")
                        }
                        let _ = get_or_update_game_dir(Some(try_path));
                    } else {
                        let err = format!("{} files not found in:\n\"{}\"", not_found.join("\n"), try_path.display());
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
    ui.global::<MainLogic>().on_toggle_mod({
        let ui_handle = ui.as_weak();
        move |key, state| -> bool {
            let ui = ui_handle.unwrap();
            let ini_dir = get_ini_dir();
            let mut ini = match Cfg::read(ini_dir) {
                Ok(ini_data) => ini_data,
                Err(err) => {
                    ui.display_msg(&err.to_string());
                    return !state;
                }
            };
            let game_dir = get_or_update_game_dir(None);
            let format_key = key.replace(' ', "_");
            match ini.collect_mods(None, false) {
                Ok(reg_mods) => {
                    if let Some(found_mod) =
                        reg_mods.iter().find(|reg_mod| format_key == reg_mod.name)
                    {
                        let result = toggle_files(&game_dir, state, found_mod, Some(ini.path()));
                        if result.is_ok() {
                            return state;
                        }
                        ui.display_msg(&result.unwrap_err().to_string());
                    } else {
                        error!("Mod: \"{key}\" not found");
                        ui.display_msg(&format!("Mod: \"{key}\" not found"))
                    };
                }
                Err(err) => ui.display_msg(&err.to_string()),
            }
            ini.update().unwrap_or_else(|err| {
                ui.display_msg(&err.to_string());
                ini = Cfg::default(ini_dir);
            });
            let order_data = order_data_or_default(ui.as_weak(), None);
            deserialize_current_mods(
                &ini.collect_mods(Some(&order_data), false).unwrap_or_else(|_| {
                    ini.collect_mods( Some(&order_data), false).unwrap_or_else(|err| {
                        ui.display_msg(&err.to_string());
                            vec![RegMod::default()]
                    })
                }),ui.as_weak()
            );
            !state
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
        move |key| {
            let ui = ui_handle.unwrap();
            let ini_dir = get_ini_dir();
            let mut ini = match Cfg::read(ini_dir) {
                Ok(ini_data) => ini_data,
                Err(err) => {
                    ui.display_msg(&err.to_string());
                    return;
                }
            };
            let registered_mods = match ini.collect_mods(None, false) {
                Ok(data) => data,
                Err(err) => {
                    ui.display_msg(&err.to_string());
                    return;
                }
            };
            slint::spawn_local(async move {
                let game_dir = get_or_update_game_dir(None);
                let format_key = key.replace(' ', "_");
                let file_paths = match get_user_files(&game_dir, ui.as_weak()) {
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
                                match install_new_files_to_mod(found_mod, err.err_paths_long, &game_dir, ui_handle).await {
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
                        let num_files = files.len();
                        let mut new_data = found_mod.files.dll.clone();
                        new_data.extend(files);
                        let mut results = Vec::with_capacity(3);
                        let new_data_refs = found_mod.files.add_other_files_to_files(&new_data);
                        if found_mod.files.len() == 1 {
                            results.push(remove_entry(
                                ini.path(),
                                INI_SECTIONS[3],
                                &found_mod.name,
                            ));
                        } else {
                            results.push(remove_array(ini.path(), &found_mod.name));
                        }
                        results.push(save_paths(ini.path(), INI_SECTIONS[3], &found_mod.name, &new_data_refs));
                        if let Some(err) = results.iter().find_map(|result| result.as_ref().err()) {
                            ui.display_msg(&err.to_string());
                            let _ = remove_entry(
                                ini.path(),
                                INI_SECTIONS[2],
                                &format_key,
                            );
                        }
                        let new_data_owned = new_data_refs.iter().map(PathBuf::from).collect();
                        let updated_mod = RegMod::new(&found_mod.name, found_mod.state, new_data_owned);
                        
                        updated_mod
                            .verify_state(&game_dir, ini.path())
                            .unwrap_or_else(|err| {
                                if updated_mod
                                    .verify_state(&game_dir, ini.path())
                                    .is_err()
                                {
                                    ui.display_msg(&err.to_string());
                                    let _ = remove_entry(
                                        ini.path(),
                                        INI_SECTIONS[2],
                                        &updated_mod.name,
                                    );
                                };
                                results.push(Err(err));
                            });
                        if !results.iter().any(|r| r.is_err()) {
                            ui.display_msg(&format!("Sucessfully added {} file(s) to {}", num_files, format_key));
                        }
                        ini.update().unwrap_or_else(|err| {
                            ui.display_msg(&err.to_string());
                            ini = Cfg::default(ini_dir);
                        });
                        let order_data = order_data_or_default(ui.as_weak(), None);
                        deserialize_current_mods(
                            &ini.collect_mods(Some(&order_data), false).unwrap_or_else(|_| {
                                ini.collect_mods( Some(&order_data), false).unwrap_or_else(|err| {
                                    ui.display_msg(&err.to_string());
                                        vec![RegMod::default()]
                                })
                            }),ui.as_weak()
                        );
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
        move |key| {
            let ui = ui_handle.unwrap();
            let ini_dir = get_ini_dir();
            let mut ini = match Cfg::read(ini_dir) {
                Ok(ini_data) => ini_data,
                Err(err) => {
                    ui.display_msg(&err.to_string());
                    return;
                }
            };
            let format_key = key.replace(' ', "_");
            ui.display_confirm(&format!("Are you sure you want to de-register: \"{key}\""), false);
            slint::spawn_local(async move {
                if receive_msg().await != Message::Confirm {
                    return
                }
                let order_map: Option<HashMap<String, usize>>;
                let loader_dir = get_loader_ini_dir();
                let loader = match ModLoaderCfg::read_section(loader_dir, LOADER_SECTIONS[1]) {
                    Ok(mut data) => {
                        order_map = data.parse_section().ok();
                        data
                    },
                    Err(err) => {
                        ui.display_msg(&err.to_string());
                        order_map = None;
                        ModLoaderCfg::default(loader_dir)
                    }
                };
                let mut reg_mods = match ini.collect_mods(order_map.as_ref(), false) {
                    Ok(reg_mods) => reg_mods,
                    Err(err) => {
                        ui.display_msg(&err.to_string());
                        return;
                    }
                };
                let game_dir = get_or_update_game_dir(None);
                if let Some(found_mod) =
                    reg_mods.iter_mut().find(|reg_mod| format_key == reg_mod.name)
                {
                    if found_mod.files.dll.iter().any(FileData::is_disabled) {
                        match toggle_files(&game_dir, true, found_mod, Some(ini_dir)) {
                            Ok(files) => {
                                found_mod.files.dll = files;
                                found_mod.state = true;
                            },
                            Err(err) => {
                                ui.display_msg(&format!("Failed to set mod to enabled state on removal\naborted before removal\n\n{err}"));
                                return;
                            }
                        }
                    }
                    // we can let sync keys take care of removing files from ini
                    remove_entry(ini_dir, INI_SECTIONS[2], &found_mod.name)
                        .unwrap_or_else(|err| ui.display_msg(&err.to_string()));
                    let ui_handle = ui.as_weak();
                    match confirm_remove_mod(ui_handle, &game_dir, loader.path(), found_mod).await {
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
                ini.update().unwrap_or_else(|err| {
                    ui.display_msg(&err.to_string());
                    ini = Cfg::default(ini_dir);
                });
                let order_data = order_data_or_default(ui.as_weak(), None);
                deserialize_current_mods(
                    &ini.collect_mods(Some(&order_data), false).unwrap_or_else(|_| {
                        ini.collect_mods( Some(&order_data), false).unwrap_or_else(|err| {
                            ui.display_msg(&err.to_string());
                                vec![RegMod::default()]
                        })
                    }),ui.as_weak()
                );
            }).unwrap();
        }
    });
    ui.global::<SettingsLogic>().on_toggle_theme({
        let ui_handle = ui.as_weak();
        move |state| {
            let ui = ui_handle.unwrap();
            let current_ini = get_ini_dir();
            save_bool(current_ini, INI_SECTIONS[0], INI_KEYS[0], state).unwrap_or_else(
                |err| ui.display_msg(&format!("Failed to save theme preference\n\n{err}")),
            );
        }
    });
    ui.global::<MainLogic>().on_edit_config_item({
        let ui_handle = ui.as_weak();
        move |config_item| {
            let ui = ui_handle.unwrap();
            let game_dir = get_or_update_game_dir(None);
            let item = config_item.text.to_string();
            if !matches!(FileData::from(&item).extension, ".txt" | ".ini") {
                return;
            };
            let os_file = vec![std::ffi::OsString::from(format!("{}\\{item}", game_dir.display()))];
            open_text_files(ui.as_weak(), os_file);
        }
    });
    ui.global::<MainLogic>().on_edit_config({
        let ui_handle = ui.as_weak();
        move |config_file| {
            let ui = ui_handle.unwrap();
            let game_dir = get_or_update_game_dir(None);
            let downcast_config_file = config_file
                .as_any()
                .downcast_ref::<VecModel<SharedString>>()
                .expect("We know we set a VecModel earlier");
            let os_files = downcast_config_file
                .iter()
                .map(|path| std::ffi::OsString::from(format!("{}\\{path}", game_dir.display())))
                .collect::<Vec<_>>();
            open_text_files(ui.as_weak(), os_files);
        }
    });
    ui.global::<SettingsLogic>().on_toggle_terminal({
        let ui_handle = ui.as_weak();
        move |state| -> bool {
            let ui = ui_handle.unwrap();
            let value = if state { "1" } else { "0" };
            let ext_ini = get_loader_ini_dir();
            let mut result = state;
            save_value_ext(ext_ini, LOADER_SECTIONS[0], LOADER_KEYS[1], value).unwrap_or_else(
                |err| {
                    ui.display_msg(&err.to_string());
                    result = !state;
                },
            );
            result
        }
    });
    ui.global::<SettingsLogic>().on_set_load_delay({
        let ui_handle = ui.as_weak();
        move |time| {
            let ui = ui_handle.unwrap();
            let ext_ini = get_loader_ini_dir();
            ui.global::<MainLogic>().invoke_force_app_focus();
            if let Err(err) = save_value_ext(ext_ini, LOADER_SECTIONS[0], LOADER_KEYS[0], &time) {
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
        move |state| -> bool {
            let ui = ui_handle.unwrap();
            let game_dir = get_or_update_game_dir(None);
            let files = if state {
                vec![PathBuf::from(LOADER_FILES[1])]
            } else {
                vec![PathBuf::from(LOADER_FILES[0])]
            };
            let main_dll = RegMod::new("main", !state, files);
            match toggle_files(&game_dir, !state, &main_dll, None) {
                Ok(_) => state,
                Err(err) => {
                    ui.display_msg(&format!("{err}"));
                    !state
                }
            }
        }
    });
    ui.global::<SettingsLogic>().on_open_game_dir({
        let ui_handle = ui.as_weak();
        move || {
            let ui = ui_handle.unwrap();
            let jh = std::thread::spawn(move || {
                let game_dir = get_or_update_game_dir(None);
                std::process::Command::new("explorer").arg(game_dir.as_path()).spawn()
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
            let key = GLOBAL_NUM_KEY.load(Ordering::Acquire);
            message_sender
                .send(MessageData { message, key })
                .unwrap_or_else(|err| error!("{err}"));
        }
    });
    ui.global::<SettingsLogic>().on_scan_for_mods({
        let ui_handle = ui.as_weak();
        move || {
            let ui = ui_handle.unwrap();
            slint::spawn_local(async move {
                let game_dir = get_or_update_game_dir(None);
                if let Err(err) = confirm_scan_mods(ui.as_weak(), &game_dir, None).await {
                    ui.display_msg(&err.to_string());
                };
            }).unwrap();
        }
    });
    ui.global::<MainLogic>().on_add_remove_order({
        let ui_handle = ui.as_weak();
        move |state, key, value| -> i32 {
            let ui = ui_handle.unwrap();
            let error = 42069_i32;
            let cfg_dir = get_loader_ini_dir();
            let result: i32 = if state { 1 } else { -1 };
            let mut load_order = match ModLoaderCfg::read_section(cfg_dir, LOADER_SECTIONS[1]) {
                Ok(data) => data,
                Err(err) => {
                    ui.display_msg(&err.to_string());
                    return error;
                }
            };
            let load_orders = load_order.mut_section();
            let stable_k = match state {
                true => {
                    load_orders.insert(&key, &value.to_string());
                    Some(key.as_str())
                }
                false => {
                    if !load_orders.contains_key(&key) {
                        return error;
                    }
                    load_orders.remove(&key);
                    None
                }
            };
            if let Err(err) = load_order.update_order_entries(stable_k) {
                ui.display_msg(&format!("Failed to write to \"mod_loader_config.ini\"\n{err}"));
                return error;
            };
            let model = ui.global::<MainLogic>().get_current_mods();
            let mut selected_mod = model.row_data(value as usize).unwrap();
            selected_mod.order.set = state;
            if !state {
                selected_mod.order.at = 0;
                if selected_mod.dll_files.row_count() != 1 {
                    selected_mod.order.i = -1;
                }
            }
            model.set_row_data(value as usize, selected_mod);
            if let Err(err) = model.update_order(&mut load_order,  value, ui.as_weak()) {
                ui.display_msg(&err.to_string());
                return error;
            };
            result
        }   
    });
    ui.global::<MainLogic>().on_modify_order({
        let ui_handle = ui.as_weak();
        move |to_k, from_k, value, row, dll_i| -> i32 {
            let ui = ui_handle.unwrap();
            let mut result = 0_i32;
            let cfg_dir = get_loader_ini_dir();
            let mut load_order = match ModLoaderCfg::read_section(cfg_dir, LOADER_SECTIONS[1]) {
                Ok(data) => data,
                Err(err) => {
                    ui.display_msg(&err.to_string());
                    return -1;
                }
            };
            let load_orders = load_order.mut_section();
            if to_k != from_k && load_orders.contains_key(&from_k) {
                load_orders.remove(&from_k);
                load_orders.append(&to_k, value.to_string())
            } else if load_orders.contains_key(&to_k) {
                load_orders.insert(&to_k, value.to_string())
            } else {
                load_orders.append(&to_k, value.to_string());
                result = 1
            };
            
            load_order.update_order_entries(Some(&to_k)).unwrap_or_else(|err| {
                ui.display_msg(&format!("Failed to write to \"mod_loader_config.ini\"\n{err}"));
                if !result.is_negative() { result = -1 }
            });
            if to_k != from_k {
                let model = ui.global::<MainLogic>().get_current_mods();
                let mut selected_mod = model.row_data(row as usize).unwrap();
                selected_mod.order.i = dll_i;
                if !selected_mod.order.set { selected_mod.order.set = true }
                model.set_row_data(row as usize, selected_mod);
                if value != row {
                    if let Err(err) = model.update_order(&mut load_order, row, ui.as_weak()) {
                        ui.display_msg(&err.to_string());
                        return -1;
                    };
                }
            } else if value != row {
                let model = ui.global::<MainLogic>().get_current_mods();
                let mut curr_row = model.row_data(row as usize).unwrap();
                let mut replace_row = model.row_data(value as usize).unwrap();
                std::mem::swap(&mut curr_row.order.at, &mut replace_row.order.at);
                model.set_row_data(row as usize, replace_row);
                model.set_row_data(value as usize, curr_row);
                ui.invoke_update_mod_index(value, 1);
                ui.invoke_redraw_checkboxes();
            }
            result
        }
    });
    ui.global::<MainLogic>().on_force_deserialize({
        let ui_handle = ui.as_weak();
        move || {
            let ui = ui_handle.unwrap();
            let ini = match Cfg::read(get_ini_dir()) {
                Ok(ini_data) => ini_data,
                Err(err) => {
                    ui.display_msg(&err.to_string());
                    return;
                }
            };
            let order_data = order_data_or_default(ui.as_weak(), None);
            deserialize_current_mods(
                &ini.collect_mods(Some(&order_data), false).unwrap_or_else(|_| {
                    ini.collect_mods( Some(&order_data), false).unwrap_or_else(|err| {
                        ui.display_msg(&err.to_string());
                            vec![RegMod::default()]
                    })
                }),ui.as_weak()
            );
            info!("deserialized after encountered error");
        }
    });

    ui.invoke_focus_app();
    ui.run()
}

trait Sortable {
    fn update_order(&self, cfg: &mut ModLoaderCfg, selected_row: i32, ui_handle: slint::Weak<App>) -> std::io::Result<()>;
}

impl Sortable for ModelRc<DisplayMod> {
    fn update_order(&self, cfg: &mut ModLoaderCfg, selected_row: i32, ui_handle: slint::Weak<App>) -> std::io::Result<()> {
        let ui = ui_handle.unwrap();
        let order_map = cfg.parse_section()?;

        let mut unsorted_idx = (0..self.row_count()).collect::<Vec<_>>();
        let selected_key = self.row_data(selected_row as usize).expect("front end gives us a valid row").name;
        let mut i = 0_usize;
        let mut selected_i = 0_usize;
        let mut no_order_count = 0_usize;
        let mut seen_names = HashSet::new();
        while !unsorted_idx.is_empty() {
            if i >= unsorted_idx.len() {
                i = 0
            }
            let unsorted_i = unsorted_idx[i];
            let mut curr_row = self.row_data(unsorted_i).expect("unsorted_idx is valid ranges");
            let curr_key = curr_row.dll_files.row_data(curr_row.order.i as usize);
            let new_order: Option<&usize>;
            if curr_key.is_some() && {new_order = order_map.get(&curr_key.unwrap().to_string()); new_order}.is_some() {
                let new_order = new_order.unwrap();
                curr_row.order.at = *new_order as i32 + 1;
                if curr_row.name == selected_key {
                    selected_i = *new_order;
                }
                if unsorted_i == *new_order {
                    self.set_row_data(*new_order, curr_row);
                    unsorted_idx.swap_remove(i);
                    continue;
                }
                if let Some(index) = unsorted_idx.iter().position(|&x| x == *new_order) {
                    let swap_row = self.row_data(*new_order).unwrap();
                    if swap_row.name == selected_key {
                        selected_i = unsorted_i;
                    }
                    self.set_row_data(*new_order, curr_row);
                    self.set_row_data(unsorted_i, swap_row);
                    unsorted_idx.swap_remove(index);
                    continue;
                }
            }
            if curr_row.name == selected_key {
                selected_i = unsorted_i;
            }
            if !seen_names.contains(&curr_row.name) {
                seen_names.insert(curr_row.name.clone());
                no_order_count += 1;
            }
            if no_order_count >= unsorted_idx.len() {
                // alphabetical sort would go here
                break;
            }
            i += 1;
        }
        ui.invoke_update_mod_index(selected_i as i32, 1);
        ui.invoke_redraw_checkboxes();
        Ok(())
    }
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

async fn receive_msg() -> Message {
    let key = GLOBAL_NUM_KEY.fetch_add(1, Ordering::SeqCst) + 1;
    let mut message = Message::Esc;
    let mut receiver = RECEIVER.get().unwrap().write().await;
    while let Some(msg) = receiver.recv().await {
        if msg.key == key {
            message = msg.message;
            break;
        }
    }
    message
}

// MARK: FIXME
// Need a stable file dialog before release
// rfd will hang if user decides to create new folders or files, or select the dropdown on "open"

fn get_user_folder(path: &Path, ui_handle: slint::Weak<App>) -> std::io::Result<PathBuf> {
    let ui = ui_handle.unwrap();
    ui.window().with_winit_window(|win| -> std::io::Result<PathBuf> {
        match rfd::FileDialog::new().set_directory(path).set_parent(&win.window_handle().unwrap()).pick_folder() {
            Some(file) => Ok(file),
            None => new_io_error!(ErrorKind::InvalidInput, "No Path Selected"),
        }
    }).unwrap()
}

fn get_user_files(path: &Path, ui_handle: slint::Weak<App>) -> std::io::Result<Vec<PathBuf>> {
    let ui = ui_handle.unwrap();
    ui.window().with_winit_window(|win| -> std::io::Result<Vec<PathBuf>> {
        match rfd::FileDialog::new().set_directory(path).set_parent(&win.window_handle().unwrap()).pick_files() {
            Some(files) => match files.len() {
                0 => new_io_error!(ErrorKind::InvalidInput, "No Files Selected"),
                _ => {
                    if files.iter().any(|file| {
                        RESTRICTED_FILES.get().unwrap().iter().any(|&restricted_file| {
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
    }).unwrap()
}

fn get_ini_dir() -> &'static PathBuf {
    static CONFIG_PATH: OnceLock<PathBuf> = OnceLock::new();
    CONFIG_PATH.get_or_init(|| {
        let exe_dir = std::env::current_dir().expect("Failed to get current dir");
        exe_dir.join(INI_NAME)
    })
}

fn get_loader_ini_dir() -> &'static PathBuf {
    static LOADER_CONFIG_PATH: OnceLock<PathBuf> = OnceLock::new();
    LOADER_CONFIG_PATH.get_or_init(|| {
        get_or_update_game_dir(None).join(LOADER_FILES[2])
    })
}

fn get_or_update_game_dir(update: Option<PathBuf>) -> tokio::sync::RwLockReadGuard<'static, std::path::PathBuf> {
    static GAME_DIR: OnceLock<RwLock<PathBuf>> = OnceLock::new();

    if let Some(path) = update {
        let gd = GAME_DIR.get_or_init(|| {
            RwLock::new(PathBuf::new())
        });
        let mut gd_lock = gd.blocking_write();
        *gd_lock = path;
    }

    GAME_DIR.get().unwrap().blocking_read()
}

fn populate_restricted_files() -> [&'static OsStr; 6] {
    let mut restricted_files: [&OsStr; 6] = [OsStr::new(""); 6];
    for (i, file) in LOADER_FILES.iter().map(OsStr::new).enumerate() {
        restricted_files[i] = file;
    }
    for (i, file) in REQUIRED_GAME_FILES.iter().map(OsStr::new).enumerate() {
        restricted_files[i + LOADER_FILES.len()] = file;
    }

    restricted_files
}

fn open_text_files(ui_handle: slint::Weak<App>, files: Vec<std::ffi::OsString>) {
    let ui = ui_handle.unwrap();
    for file in files {
        let file_clone = file.clone();
        let jh = std::thread::spawn(move || {
            std::process::Command::new("notepad")
                .arg(&file)
                .spawn()
        });
        match jh.join() {
            Ok(result) => match result {
                Ok(_) => (),
                Err(err) => {
                    error!("{err}");
                    ui.display_msg(&format!(
                        "Failed to open config file {file_clone:?}\n\nError: {err}"
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

fn order_data_or_default(ui_handle: slint::Weak<App>, from_path: Option<&Path>) -> HashMap<String, usize> {
    let ui = ui_handle.unwrap();
    let path: &Path;
    if let Some(dir) = from_path {
        path = dir
    } else { path = get_loader_ini_dir() };
    match ModLoaderCfg::read_section(path, LOADER_SECTIONS[1]) {
        Ok(mut data) => {
            data.parse_section().unwrap_or_else(|err| {
                ui.display_msg(&err.to_string());
                HashMap::new()
            })
        },
        Err(err) => {
            ui.display_msg(&err.to_string());
            HashMap::new()
        }
    }
}

fn deserialize_current_mods(mods: &[RegMod], ui_handle: slint::Weak<App>) {
    let ui = ui_handle.unwrap();
    let display_mods: Rc<VecModel<DisplayMod>> = Default::default();
    for mod_data in mods.iter() {
        let files: Rc<VecModel<slint::StandardListViewItem>> = Default::default();
        let dll_files: Rc<VecModel<SharedString>> = Default::default();
        let config_files: Rc<VecModel<SharedString>> = Default::default();
        if !mod_data.files.dll.is_empty() {
            files.extend(mod_data.files.dll.iter().map(|f| SharedString::from(f.to_string_lossy().replace(OFF_STATE, "")).into()));
            dll_files.extend(mod_data.files.dll.iter().map(|f| SharedString::from(f.file_name().unwrap().to_string_lossy().replace(OFF_STATE, ""))));
        };
        if !mod_data.files.config.is_empty() {
            files.extend(mod_data.files.config.iter().map(|f| SharedString::from(f.to_string_lossy().to_string()).into()));
            config_files.extend(mod_data.files.config.iter().map(|f| SharedString::from(f.to_string_lossy().to_string())));
        };
        if !mod_data.files.other.is_empty() {
            files.extend(mod_data.files.other.iter().map(|f| SharedString::from(f.to_string_lossy().to_string()).into()));
        };
        let name = mod_data.name.replace('_', " ");
        display_mods.push(DisplayMod {
            displayname: SharedString::from(if mod_data.name.chars().count() > 20 {
                format!("{}...", &name[..17])
            } else {
                name.clone()
            }),
            name: SharedString::from(name),
            enabled: mod_data.state,
            files: ModelRc::from(files),
            config_files: ModelRc::from(config_files),
            dll_files: ModelRc::from(dll_files),
            order: LoadOrder { 
                at: if !mod_data.order.set { 0 } else { mod_data.order.at as i32 + 1 }, 
                i: if !mod_data.order.set && mod_data.files.dll.len() != 1 { -1 } else { mod_data.order.i as i32 },
                set: mod_data.order.set 
            },
        })
    }
    ui.global::<MainLogic>().set_current_mods(ModelRc::from(display_mods));
    ui.global::<MainLogic>().set_orders_set(mods.order_count() as i32);
}

async fn install_new_mod(
    name: &str,
    files: Vec<PathBuf>,
    game_dir: &Path,
    ui_handle: slint::Weak<App>,
) -> std::io::Result<Vec<PathBuf>> {
    let ui = ui_handle.unwrap();
    let mod_name = name.trim();
    ui.display_confirm(
        &format!(
            "Mod files are not installed in game directory.\nAttempt to install \"{mod_name}\"?"
        ),
        true,
    );
    if receive_msg().await != Message::Confirm {
        return new_io_error!(ErrorKind::ConnectionAborted, "Mod install canceled");
    }
    match InstallData::new(mod_name, files, game_dir) {
        Ok(data) => add_dir_to_install_data(data, ui_handle).await,
        Err(err) => Err(err)
    }
}

async fn install_new_files_to_mod(
    mod_data: &RegMod,
    files: Vec<PathBuf>,
    game_dir: &Path,
    ui_handle: slint::Weak<App>,
) -> std::io::Result<Vec<PathBuf>> {
    let ui = ui_handle.unwrap();
    ui.display_confirm("Selected files are not installed? Would you like to try and install them?", true);
    if receive_msg().await != Message::Confirm {
        return new_io_error!(ErrorKind::ConnectionAborted, "Did not select to install files");
    };
    match InstallData::amend(mod_data, files, game_dir) {
        Ok(data) => confirm_install(data, ui_handle).await,
        Err(err) => Err(err)
    }
}

async fn add_dir_to_install_data(
    mut install_files: InstallData,
    ui_handle: slint::Weak<App>,
) -> std::io::Result<Vec<PathBuf>> {
    let ui = ui_handle.unwrap();
    ui.display_confirm(&format!(
        "Current Files to install:\n{}\n\nWould you like to add a directory eg. Folder containing a config file?", 
        install_files.display_paths), true);
    let mut result: Vec<Result<(), std::io::Error>> = Vec::with_capacity(2);
    match receive_msg().await {
        Message::Confirm => match get_user_folder(&install_files.parent_dir, ui.as_weak()) {
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
                let _ = receive_msg().await;
                let reselect_dir = Box::pin(async {
                    add_dir_to_install_data(install_files, ui_handle).await
                });
                reselect_dir.await
            } else {
                new_io_error!(ErrorKind::Other, format!("Error: Could not Install\n\n{err}"))
            }
        }
        true => confirm_install(install_files, ui_handle).await,
    }
}

async fn confirm_install(
    install_files: InstallData,
    ui_handle: slint::Weak<App>,
) -> std::io::Result<Vec<PathBuf>> {
    let ui = ui_handle.unwrap();
    ui.display_confirm(
        &format!(
            "Confirm install of mod \"{}\"\n\nSelected files:\n{}\n\nInstall at:\n{}",
            install_files.name,
            install_files.display_paths,
            &install_files.install_dir.display()
        ),
        false,
    );
    if receive_msg().await != Message::Confirm {
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
    ui_handle: slint::Weak<App>,
    game_dir: &Path, loader_dir: &Path, reg_mod: &RegMod) -> std::io::Result<()> {
    let ui = ui_handle.unwrap();
    let install_dir = match reg_mod.files.file_refs().iter().min_by_key(|file| file.ancestors().count()) {
        Some(path) => game_dir.join(parent_or_err(path)?),
        None => PathBuf::from("Error: Failed to display a parent_dir"),
    };
    ui.display_confirm("Do you want to remove mod files from the game directory?", true);
    if receive_msg().await != Message::Confirm {
        return new_io_error!(ErrorKind::ConnectionAborted, format!("Mod files are still installed at \"{}\"", install_dir.display()));
    };
    ui.display_confirm("This is a distructive action. Are you sure you want to continue?", false);
    if receive_msg().await != Message::Confirm {
        return new_io_error!(ErrorKind::ConnectionAborted, format!("Mod files are still installed at \"{}\"", install_dir.display()));
    };
    remove_mod_files(game_dir, loader_dir, reg_mod)
}

async fn confirm_scan_mods(
    ui_handle: slint::Weak<App>,
    game_dir: &Path,
    ini: Option<Cfg>) -> std::io::Result<()> {
    let ui = ui_handle.unwrap();
    
    ui.display_confirm("Would you like to attempt to auto-import already installed mods to Elden Mod Loader GUI?", true);
    if receive_msg().await != Message::Confirm {
        return Ok(());
    };
    
    let ini = match ini {
        Some(data) => data,
        None => Cfg::read(get_ini_dir())?,
    };
    let order_map: Option<HashMap<String, usize>>;
    let loader_dir = get_loader_ini_dir();
    let loader = match ModLoaderCfg::read_section(loader_dir, LOADER_SECTIONS[1]) {
        Ok(mut data) => {
            order_map = data.parse_section().ok();
            data
        },
        Err(err) => {
            ui.display_msg(&err.to_string());
            order_map = None;
            ModLoaderCfg::default(loader_dir)
        }
    };

    let mut old_mods: Vec<RegMod>;
    if !ini.mods_empty() {
        ui.display_confirm("Warning: This action will reset current registered mods, are you sure you want to continue?", true);
        if receive_msg().await != Message::Confirm {
            return Ok(());
        };
        old_mods = ini.collect_mods(order_map.as_ref(), false)?;
        let dark_mode = ui.global::<SettingsLogic>().get_dark_mode();

        std::fs::remove_file(ini.path())?;
        new_cfg(ini.path())?;
        save_bool(ini.path(), INI_SECTIONS[0], INI_KEYS[0], dark_mode)?;
        save_path(ini.path(), INI_SECTIONS[1], INI_KEYS[1], game_dir)?;
    } else {
        old_mods = Vec::new();
    }
    let new_ini: Cfg;
    match scan_for_mods(game_dir, ini.path()) {
        Ok(len) => {
            new_ini = match Cfg::read(ini.path()) {
                Ok(ini_data) => ini_data,
                Err(err) => {
                    ui.display_msg(&err.to_string());
                    return Err(err);
                }
            };
            ui.global::<MainLogic>().set_current_subpage(0);
            let mod_loader = ModLoader::properties(game_dir).unwrap_or_default();
            let order_data = order_data_or_default(ui.as_weak(), Some(mod_loader.path()));
            deserialize_current_mods(
                &new_ini.collect_mods(Some(&order_data), false).unwrap_or_else(|_| {
                    new_ini.collect_mods( Some(&order_data), false).unwrap_or_else(|err| {
                        ui.display_msg(&err.to_string());
                            vec![RegMod::default()]
                    })
                }),ui.as_weak()
            );
            ui.display_msg(&format!("Successfully Found {len} mod(s)"));
        }
        Err(err) => {
            ui.display_msg(&format!("Error: {err}"));
            new_ini = Cfg::default(ini.path());
        },
    };
    if !old_mods.is_empty() {
        let new_mods = new_ini.collect_mods(None, false)?;
        let all_new_files = new_mods.iter().flat_map(|m| m.files.file_refs()).collect::<HashSet<_>>();
        old_mods.retain(|m| m.files.dll.iter().any(|f| !all_new_files.contains(f.as_path())));
        if old_mods.is_empty() { return Ok(()) }

        // unsure if we want to remove order data, currently on mod removal we do not delete order data,
        // we only delete order data on mod uninstallation
        old_mods.iter().try_for_each(|m| {
            if m.order.set && !all_new_files.contains(m.files.dll[m.order.i].as_path()) {
                remove_order_entry(m, loader.path())
            } else {
                Ok(())
            }
        })?;

        old_mods.iter_mut().for_each(|m| m.files.dll.retain(|f| !all_new_files.contains(f.as_path())));
        old_mods.retain(|m| !m.files.dll.is_empty());
        if old_mods.is_empty() { return Ok(()) }
        old_mods.retain(|m| m.files.dll.iter().any(FileData::is_disabled));
        if old_mods.is_empty() { return Ok(()) }

        old_mods.iter().try_for_each(|m| toggle_files(game_dir, true, m, None).map(|_| ()))?;
    }
    Ok(())
}
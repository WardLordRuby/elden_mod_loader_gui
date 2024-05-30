#![cfg(target_os = "windows")]
// Setting windows_subsystem will hide console | cant read logs if console is hidden
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use elden_mod_loader_gui::{
    utils::{
        ini::{
            common::*,
            mod_loader::{Countable, ModLoader, NameSet},
            parser::{CollectedMods, RegMod, Setup, SplitFiles},
            writer::*,
        },
        installer::{remove_mod_files, scan_for_mods, InstallData},
        subscriber::init_subscriber,
    },
    *,
};
use i_slint_backend_winit::WinitWindowAccessor;
use slint::{ComponentHandle, Model, ModelRc, SharedString, StandardListViewItem, Timer, VecModel};
use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    io::ErrorKind,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        atomic::{AtomicU32, Ordering},
        OnceLock,
    },
};
use tokio::sync::{
    mpsc::{unbounded_channel, UnboundedReceiver},
    RwLock,
};
use tracing::{error, info, info_span, instrument, trace, warn};

slint::include_modules!();

static GLOBAL_NUM_KEY: AtomicU32 = AtomicU32::new(0);
static RESTRICTED_FILES: OnceLock<HashSet<&OsStr>> = OnceLock::new();
static RECEIVER: OnceLock<RwLock<UnboundedReceiver<MessageData>>> = OnceLock::new();

fn main() -> Result<(), slint::PlatformError> {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        error!(name: "PANIC", "{}", format_panic_info(info));
        prev(info);
    }));

    let mut errors = Vec::new();
    let _guard = init_subscriber().unwrap_or_else(|err| {
        errors.push(err);
        None
    });

    slint::platform::set_platform(Box::new(
        i_slint_backend_winit::Backend::new().expect("This app is being run on windows"),
    ))
    .expect("This app uses the winit backend");
    let ui = App::new()?;
    ui.window().with_winit_window(|window: &winit::window::Window| {
        window.set_enabled_buttons(
            winit::window::WindowButtons::CLOSE | winit::window::WindowButtons::MINIMIZE,
        );
    });
    let (message_sender, message_receiver) = unbounded_channel::<MessageData>();
    RECEIVER.set(RwLock::new(message_receiver)).unwrap();
    RESTRICTED_FILES.set(populate_restricted_files()).unwrap();
    {
        let span = info_span!("startup");
        let _gaurd = span.enter();

        let current_ini = get_ini_dir();
        let first_startup: bool;
        let ini = match current_ini.is_setup(&INI_SECTIONS) {
            Ok(ini_data) => {
                first_startup = false;
                Some(ini_data)
            }
            Err(err) => {
                // MARK: TODO
                // create paths for these different error cases
                first_startup = matches!(
                    err.kind(),
                    ErrorKind::NotFound | ErrorKind::PermissionDenied | ErrorKind::InvalidData
                );
                if err.kind() == ErrorKind::InvalidInput {
                    error!(err_code = 1, "{err}")
                }
                if !first_startup || err.kind() == ErrorKind::InvalidData {
                    errors.push(err)
                }
                None
            }
        };
        let mut ini = match ini {
            Some(ini_data) => Config::from(ini_data, current_ini),
            None => {
                Cfg::read(current_ini).unwrap_or_else(|err| {
                    // io::write error
                    error!(err_code = 2, "{err}");
                    errors.push(err);
                    Cfg::default(current_ini)
                })
            }
        };

        let game_verified: bool;
        let mod_loader: ModLoader;
        let mut mod_loader_cfg: ModLoaderCfg;
        let mut reg_mods = None;
        let mut order_data = None;
        let game_dir = match ini.attempt_locate_game() {
            Ok(PathResult::Full(path)) => {
                mod_loader = ModLoader::properties(&path).unwrap_or_else(|err| {
                    error!(err_code = 3, "{err}");
                    errors.push(err);
                    ModLoader::default()
                });
                if mod_loader.installed() {
                    info!(dll_hook = %DisplayState(!mod_loader.disabled()), "elden_mod_loader files found");
                    mod_loader_cfg = ModLoaderCfg::read(mod_loader.path()).unwrap_or_else(|err| {
                        error!(err_code = 4, "{err}");
                        errors.push(err);
                        ModLoaderCfg::default(mod_loader.path())
                    });
                    order_data = match mod_loader_cfg.parse_section() {
                        Ok(data) => Some(data),
                        Err(err) => {
                            error!(err_code = 5, "{err}");
                            errors.push(err);
                            None
                        }
                    };
                } else {
                    mod_loader_cfg = ModLoaderCfg::default(mod_loader.path());
                }

                reg_mods = {
                    let mut collection = ini.collect_mods(&path, order_data.as_ref(), false);
                    let dlls = collection.mods.dll_name_set();
                    if mod_loader.installed() {
                        if let Err(err) =
                            mod_loader_cfg.verify_keys(&dlls, collection.mods.order_count())
                        {
                            match err.kind() {
                                ErrorKind::Unsupported => {
                                    order_data = Some(mod_loader_cfg.parse_into_map());
                                    ini.update().unwrap_or_else(|err| {
                                        error!(err_code = 6, "{err}");
                                        ui.display_msg(&err.to_string());
                                    });
                                    collection =
                                        ini.collect_mods(&path, order_data.as_ref(), false);
                                    warn!("{err}");
                                }
                                ErrorKind::Other => info!("{err}"),
                                _ => error!(err_code = 7, "{err}"),
                            }
                            errors.push(err);
                        }
                    }
                    debug_assert_eq!(collection.mods.len(), ini.mods_registered());
                    if let Some(warning) = collection.warnings.take() {
                        errors.push(warning);
                    }
                    info!(
                        "Found {} mod(s) registered in: {}",
                        collection.mods.len(),
                        INI_NAME
                    );
                    Some(collection)
                };
                game_verified = true;
                Some(path)
            }
            Ok(PathResult::Partial(path) | PathResult::None(path)) => {
                mod_loader_cfg = ModLoaderCfg::empty();
                mod_loader = ModLoader::default();
                game_verified = false;
                Some(path)
            }
            Err(err) => {
                // io::Write error
                error!(err_code = 8, "{err}");
                errors.push(err);
                mod_loader_cfg = ModLoaderCfg::empty();
                mod_loader = ModLoader::default();
                game_verified = false;
                None
            }
        };

        ui.global::<SettingsLogic>()
            .set_dark_mode(ini.get_dark_mode().unwrap_or_else(|err| {
                // parse error ErrorKind::InvalidData
                error!(err_code = 9, "{err}");
                errors.push(err);
                DEFAULT_INI_VALUES[0]
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
        let _ = get_or_update_game_dir(Some(
            game_dir.as_ref().unwrap_or(&PathBuf::new()).to_owned(),
        ));

        if !game_verified {
            ui.global::<MainLogic>().set_current_subpage(1);
        } else {
            deserialize_collected_mods(
                &if let Some(mod_data) = reg_mods {
                    mod_data
                } else {
                    ini.collect_mods(
                        game_dir.as_ref().expect("game verified"),
                        order_data.as_ref(),
                        !mod_loader.installed(),
                    )
                },
                ui.as_weak(),
            );
            ui.global::<SettingsLogic>()
                .set_loader_disabled(mod_loader.disabled());

            if mod_loader.installed() {
                ui.global::<SettingsLogic>().set_loader_installed(true);
                let delay = mod_loader_cfg.get_load_delay().unwrap_or_else(|err| {
                    // parse error ErrorKind::InvalidData
                    error!(err_code = 10, "{err}");
                    errors.push(err);
                    DEFAULT_LOADER_VALUES[0].parse().unwrap()
                });
                let show_terminal = mod_loader_cfg.get_show_terminal().unwrap_or_else(|err| {
                    // parse error ErrorKind::InvalidData
                    error!(err_code = 11, "{err}");
                    errors.push(err);
                    false
                });

                ui.global::<SettingsLogic>()
                    .set_load_delay(SharedString::from(format!("{delay}ms")));
                ui.global::<SettingsLogic>().set_show_terminal(show_terminal);
            }
        }
        // we need to wait for slint event loop to start `ui.run()` before making calls to `ui.display_msg()`
        // otherwise calculations for the positon of display_msg_popup are not correct
        let ui_handle = ui.as_weak();
        let span_clone = span.clone();
        slint::invoke_from_event_loop(move || {
            Timer::single_shot(std::time::Duration::from_millis(200), move || {
                slint::spawn_local(async move {
                    let _gaurd = span_clone.enter();
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
                            if let Err(err) = confirm_scan_mods(ui.as_weak(), &game_dir.expect("game_verified"), Some(&ini), order_data.as_ref()).await {
                                ui.display_msg(&err.to_string());
                            };
                        }
                    } else if game_verified {
                        if !mod_loader.installed() {
                            ui.display_msg(&format!("This tool requires Elden Mod Loader by TechieW to be installed!\n\nPlease install files to \"{}\", and relaunch Elden Mod Loader GUI", get_or_update_game_dir(None).display()));
                        } else if ini.mods_is_empty() {
                            if let Err(err) = confirm_scan_mods(ui.as_weak(), &game_dir.expect("game_verified"), Some(&ini), order_data.as_ref()).await {
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

    ui.global::<MainLogic>().on_select_mod_files({
        let ui_handle = ui.as_weak();
        move |mod_name| {
            let span = info_span!("add_mod");
            let _gaurd = span.enter();

            let ui = ui_handle.unwrap();
            let ini_dir = get_ini_dir();
            let game_dir = get_or_update_game_dir(None);
            let mut ini = match Cfg::read(ini_dir) {
                Ok(ini_data) => ini_data,
                Err(err) => {
                    error!("{err}");
                    ui.display_msg(&err.to_string());
                    return;
                }
            };
            let format_key = mod_name.trim().replace(' ', "_");
            if ini.keys().contains(&format_key) {
                ui.display_msg(&format!(
                    "There is already a registered mod with the name\n\"{mod_name}\""
                ));
                ui.global::<MainLogic>()
                    .set_line_edit_text(SharedString::new());
                return;
            }
            let span_clone = span.clone();
            slint::spawn_local(async move {
                let _gaurd = span_clone.enter();
                let mut file_paths = match get_user_files(&game_dir, ui.as_weak()) {
                    Ok(files) => files,
                    Err(err) => {
                        if err.kind() != ErrorKind::InvalidInput {
                            error!("{err}");
                        }
                        ui.display_msg(&err.to_string());
                        return;
                    }
                };
                let files = match shorten_paths(&file_paths, &game_dir) {
                    Ok(files) => files,
                    Err(err) => {
                        if file_paths.len() == err.err_paths_long.len() {
                            let ui_handle = ui.as_weak();
                            match install_new_mod(&mod_name, file_paths, &game_dir, ui_handle).await {
                                Ok(installed_files) => {
                                    file_paths = installed_files;
                                    match shorten_paths(&file_paths, &game_dir) {
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
                let registered_files = ini.files();
                if files.iter().any(|f| registered_files.contains(f.to_str().unwrap_or_default())) {
                    let err_str = "A selected file is already registered to a mod";
                    error!("{err_str}");
                    ui.display_msg(err_str);
                    return;
                };
                let loader_dir = get_loader_ini_dir();
                let mut loader_cfg = ModLoaderCfg::read(loader_dir).unwrap_or_else(|err| {
                    error!("{err}");
                    ui.display_msg(&err.to_string());
                    ModLoaderCfg::default(loader_dir)
                });
                let order_data = loader_cfg.parse_section().unwrap_or_else(|err| {
                    error!("{err}");
                    ui.display_msg(&err.to_string());
                    HashMap::new()
                });
                let mut new_mod = RegMod::with_load_order(&format_key, true, files.iter().map(PathBuf::from).collect(), &order_data);
                if !new_mod.files.dll.is_empty() {
                    if new_mod.files.dll.iter().all(FileData::is_disabled) {
                        new_mod.state = false;
                    }
                    if let Err(err) = new_mod.verify_state(&game_dir, ini.path()) {
                        // Toggle files returned an error lets try it again
                        if new_mod.verify_state(&game_dir, ini.path()).is_err() {
                            ui.display_msg(&err.to_string());
                            return;
                        };
                    };
                }
                if let Err(err) = new_mod.write_to_file(ini.path(), false) {
                    let _ = new_mod.remove_from_file(ini.path());
                    error!("{err}");
                    ui.display_msg(&err.to_string());
                    return;
                };

                ui.global::<MainLogic>().set_line_edit_text(SharedString::new());
                ini.update().unwrap_or_else(|err| {
                    error!("{err}");
                    ui.display_msg(&err.to_string());
                    ini = Cfg::default(ini_dir);
                });

                let model = ui.global::<MainLogic>().get_current_mods();
                let mut_model = model.as_any().downcast_ref::<VecModel<DisplayMod>>().expect("we set this type earlier");
                mut_model.push(deserialize_mod(&new_mod));
                if new_mod.order.set {
                    model.update_order(None, &order_data, ui.as_weak());
                }
                info!(
                    files = new_mod.files.file_refs().len(),
                    state = %DisplayState(new_mod.state),
                    order = %DisplayOrder(new_mod.order.set, new_mod.order.at),
                    "{mod_name} added with"
                );
            }).unwrap();
        }
    });
    ui.global::<SettingsLogic>().on_select_game_dir({
        let ui_handle = ui.as_weak();
        move || {
            let span = info_span!("select_game_path");
            let _gaurd = span.enter();

            let ui = ui_handle.unwrap();
            let ini = match Cfg::read(get_ini_dir()) {
                Ok(ini_data) => ini_data,
                Err(err) => {
                    error!("{err}");
                    ui.display_msg(&err.to_string());
                    return;
                }
            };
            let span_clone = span.clone();
            slint::spawn_local(async move {
                let _gaurd = span_clone.enter();
                let game_dir = get_or_update_game_dir(None);
                let path_result = get_user_folder(&game_dir, ui.as_weak());
                drop(game_dir);
                let path = match path_result {
                    Ok(path) => path,
                    Err(err) => {
                        ui.display_msg(&err.to_string());
                        return;
                    }
                };
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
                        let result = save_path(ini.path(), INI_SECTIONS[1], INI_KEYS[2], &try_path);
                        if result.is_err() && save_path(ini.path(), INI_SECTIONS[1], INI_KEYS[2], &try_path).is_err() {
                            let err = result.unwrap_err();
                            error!("Failed to save directory. {err}");
                            ui.display_msg(&err.to_string());
                            return;
                        };
                        info!("game_dir saved as: \"{}\"", try_path.display());
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
                            if ini.mods_is_empty() {
                                if let Err(err) = confirm_scan_mods(ui.as_weak(), &try_path, Some(&ini), None).await {
                                    ui.display_msg(&err.to_string());
                                };
                            }
                        } else {
                            ui.display_msg("Game Files Found!\n\nCould not find Elden Mod Loader Script!\nThis tool requires Elden Mod Loader by TechieW to be installed!")
                        }
                        let _ = get_or_update_game_dir(Some(try_path));
                    } else {
                        error!("Required game files not found in: \"{}\", files missing: {}", try_path.display(), DisplayStrs(not_found));
                        ui.display_msg(&format!("Could not find Elden Ring in:\n\"{}\"", try_path.display()));
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
            let span = info_span!("toggle_mod");
            let _gaurd = span.enter();

            let ui = ui_handle.unwrap();
            let ini_dir = get_ini_dir();
            let ini = match Cfg::read(ini_dir) {
                Ok(ini_data) => ini_data,
                Err(err) => {
                    error!("{err}");
                    ui.display_msg(&err.to_string());
                    return !state;
                }
            };
            let game_dir = get_or_update_game_dir(None);
            match ini.get_mod(&key, &game_dir, None) {
                Ok(ref mut reg_mod) => {
                    if reg_mod.files.dll.is_empty() {
                        info!(
                            "Can not toggle {}, if mod has no .dll files",
                            DisplayName(&reg_mod.name)
                        );
                        ui.display_msg(&format!(
                            "To toggle \"{}\" please add a .dll file",
                            DisplayName(&reg_mod.name)
                        ));
                        return !state;
                    }
                    if let Err(err) = toggle_files(&game_dir, state, reg_mod, Some(ini.path())) {
                        error!("{err}");
                        ui.display_msg(&err.to_string());
                    } else {
                        return state;
                    };
                }
                Err(err) => {
                    error!("{err}");
                    ui.display_msg(&err.to_string());
                }
            }
            reset_app_state(ini, &game_dir, None, ui.as_weak());
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
        move |row| {
            let span = info_span!("add_to_mod");
            let _gaurd = span.enter();

            let ui = ui_handle.unwrap();
            let ini_dir = get_ini_dir();
            let game_dir = get_or_update_game_dir(None);
            let ini = match Cfg::read(ini_dir) {
                Ok(ini_data) => ini_data,
                Err(err) => {
                    error!("{err}");
                    ui.display_msg(&err.to_string());
                    return;
                }
            };
            let span_clone = span.clone();
            slint::spawn_local(async move {
                let _gaurd = span_clone.enter();
                let mut file_paths = match get_user_files(&game_dir, ui.as_weak()) {
                    Ok(paths) => paths,
                    Err(err) => {
                        if err.kind() != ErrorKind::InvalidInput {
                            error!("{err}");
                        }
                        ui.display_msg(&err.to_string());
                        return;
                    }
                };
                let model = ui.global::<MainLogic>().get_current_mods();
                let mut display_mod = model.row_data(row as usize).expect("front end gives us valid row");
                let mut found_mod = match ini.get_mod(&display_mod.name, &game_dir, None) {
                    Ok(reg_mod) => reg_mod,
                    Err(err) => {
                        error!("{err}");
                        ui.display_msg(&err.to_string());
                        reset_app_state(ini, &game_dir, None, ui.as_weak());
                        info!("deserialized after encountered error");
                        return;
                    }
                };
                let files = match shorten_paths(&file_paths, &game_dir) {
                    Ok(files) => files,
                    Err(err) => {
                        if file_paths.len() == err.err_paths_long.len() {
                            let ui_handle = ui.as_weak();
                            match install_new_files_to_mod(&found_mod, file_paths, &game_dir, ui_handle).await {
                                Ok(installed_files) => {
                                    file_paths = installed_files;
                                    match shorten_paths(&file_paths, &game_dir) {
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
                            error!(files = ?err.err_paths_long, "Encountered {} StripPrefixError(s) on input", err.err_paths_long.len());
                            ui.display_msg(&format!("Some selected files are already installed\n\nSelected Files Installed: {}\nSelected Files not installed: {}", err.ok_paths_short.len(), err.err_paths_long.len()));
                            return;
                        }
                    }
                };
                let registered_files = ini.files();
                if files.iter().any(|f| registered_files.contains(f.to_str().unwrap_or_default())) {
                    let err_str = "A selected file is already registered to a mod";
                    error!("{err_str}");
                    ui.display_msg(err_str);
                    return;
                };
                let num_files = files.len();
                let was_array = found_mod.is_array();
                files.iter().for_each(|path| found_mod.files.add(path));
                if let Err(err) = found_mod.write_to_file(ini_dir, was_array) {
                    error!("{err}");
                    ui.display_msg(&err.to_string());
                    return;
                };

                if let Err(err) = found_mod.verify_state(&game_dir, ini.path()) {
                    ui.display_msg(&err.to_string());
                    let _ = found_mod.remove_from_file(ini.path());
                    let err_str = format!("Failed to verify state, mod was removed {err}");
                    error!("{err_str}");
                    ui.display_msg(&err_str);
                    reset_app_state(ini, &game_dir, None, ui.as_weak());
                    return;
                };
                let (files, dll_files, config_files) = deserialize_split_files(&found_mod.files);
                display_mod.files = files;
                display_mod.dll_files = dll_files;
                display_mod.config_files = config_files;
                model.set_row_data(row as usize, display_mod);
                let success = format!("Added {} file(s) to: {}", num_files, DisplayName(&found_mod.name));
                info!("{success}");
                ui.display_msg(&success);
            })
            .unwrap();
        }
    });
    ui.global::<MainLogic>().on_remove_mod({
        let ui_handle = ui.as_weak();
        move |key, row| {
            let handle_clone = ui_handle.clone();
            slint::spawn_local(async move {
                let span = info_span!("remove_mod");
                let _gaurd = span.enter();
                let ui = handle_clone.unwrap();
                ui.display_confirm(&format!("Are you sure you want to de-register: {key}?"), false);
                if receive_msg().await != Message::Confirm {
                    return
                }
                let ini_dir = get_ini_dir();
                let ini = match Cfg::read(ini_dir) {
                    Ok(ini_data) => ini_data,
                    Err(err) => {
                        error!("{err}");
                        ui.display_msg(&err.to_string());
                        return;
                    }
                };
                let order_map: Option<OrderMap>;
                let loader_dir = get_loader_ini_dir();
                let mut loader = match ModLoaderCfg::read(loader_dir) {
                    Ok(mut data) => {
                        order_map = data.parse_section().ok();
                        data
                    },
                    Err(err) => {
                        error!("{err}");
                        ui.display_msg(&err.to_string());
                        order_map = None;
                        ModLoaderCfg::default(loader_dir)
                    }
                };
                let game_dir = get_or_update_game_dir(None);
                let mut reg_mods = {
                    let data = ini.collect_mods(game_dir.as_path(), order_map.as_ref(), false);
                    if let Some(warning) = data.warnings {
                        warn!("{warning}");
                        ui.display_msg(&warning.to_string());
                    }
                    data.mods
                };
                let format_key = key.replace(' ', "_");
                let Some(found_i) = reg_mods.iter().position(|reg_mod| format_key == reg_mod.name) else
                {
                    let err = &format!("Mod: {key} not found");
                    error!("{err}");
                    ui.display_msg(&format!("{err}\nRemoving invalid entries"));
                    reset_app_state(ini, &game_dir, Some(loader_dir), ui.as_weak());
                    return;
                };
                let mut found_mod = reg_mods.swap_remove(found_i);
                if found_mod.files.dll.iter().any(FileData::is_disabled) {
                    if let Err(err) = toggle_files(&game_dir, true, &mut found_mod, None) {
                        let error = format!("Failed to set mod to enabled state on removal\naborted before removal\n\n{err}");
                        error!("{error}");
                        ui.display_msg(&error);
                        return;
                    }
                }
                if let Err(err) = found_mod.remove_from_file(ini_dir) {
                    error!("{err}");
                    ui.display_msg(&err.to_string());
                    return;
                };
                match confirm_remove_mod(ui.as_weak(), &game_dir, loader.path(), &found_mod).await {
                    Ok(_) => {
                        let success = format!("{key} uninstalled, all associated files were removed");
                        info!("{success}");
                        ui.display_msg(&success);
                    },
                    Err(err) => {
                        match err.kind() {
                            ErrorKind::ConnectionAborted => info!("{err}"),
                            _ => error!("{err}"),
                        }
                        let deregister = format!("De-registered mod: {key}");
                        info!("{deregister}");
                        ui.display_msg(&deregister);
                        let _ = receive_msg().await;
                        ui.display_msg(&err.to_string())
                    }
                }
                let dlls = reg_mods.dll_name_set();
                let order_count = reg_mods.order_count();
                let model = ui.global::<MainLogic>().get_current_mods();
                let mut_model = model.as_any().downcast_ref::<VecModel<DisplayMod>>().expect("we set this type earlier");
                ui.global::<MainLogic>().set_current_subpage(0);
                mut_model.remove(row as usize);
                loader.verify_keys(&dlls, order_count).unwrap_or_else(|err| {
                    warn!("{err}");
                    ui.display_msg(&err.to_string());
                });
                let order_data = loader.parse_section().unwrap_or_else(|err| {
                    error!("{err}");
                    ui.display_msg(&err.to_string());
                    HashMap::new()
                });
                if found_mod.order.set {
                    ui.global::<MainLogic>().set_orders_set(order_count as i32);
                    model.update_order(None, &order_data, ui.as_weak());
                }
            }).unwrap();
        }
    });
    ui.global::<SettingsLogic>().on_toggle_theme({
        let ui_handle = ui.as_weak();
        move |state| {
            let span = info_span!("toggle_theme");
            let _gaurd = span.enter();
            let ui = ui_handle.unwrap();
            let current_ini = get_ini_dir();
            if let Err(err) = save_bool(current_ini, INI_SECTIONS[0], INI_KEYS[0], state) {
                let err_str = format!("Failed to save theme preference\n\n{err}");
                error!("{err_str}");
                ui.display_msg(&err_str);
            } else {
                info!("Theme set to: {}", DisplayTheme(state));
            };
        }
    });
    ui.global::<MainLogic>().on_edit_config_item({
        let ui_handle = ui.as_weak();
        move |config_item| {
            let span = info_span!("edit_config");
            let _gaurd = span.enter();

            let ui = ui_handle.unwrap();
            let game_dir = get_or_update_game_dir(None);
            let item = config_item.text.to_string();
            if !matches!(FileData::from(&item).extension, ".txt" | ".ini") {
                return;
            };
            let os_file = vec![game_dir.join(item)];
            open_text_files(ui.as_weak(), os_file);
        }
    });
    ui.global::<MainLogic>().on_edit_config({
        let ui_handle = ui.as_weak();
        move |config_file| {
            let span = info_span!("edit_config");
            let _gaurd = span.enter();

            let ui = ui_handle.unwrap();
            let game_dir = get_or_update_game_dir(None);
            let downcast_config_file = config_file
                .as_any()
                .downcast_ref::<VecModel<SharedString>>()
                .expect("We know we set a VecModel earlier");
            let os_files = downcast_config_file
                .iter()
                .map(|path| game_dir.join(path.to_string()))
                .collect::<Vec<_>>();
            open_text_files(ui.as_weak(), os_files);
        }
    });
    ui.global::<SettingsLogic>().on_toggle_terminal({
        let ui_handle = ui.as_weak();
        move |state| -> bool {
            let span = info_span!("toggle_terminal");
            let _gaurd = span.enter();

            let ui = ui_handle.unwrap();
            let value = if state { "1" } else { "0" };
            let ext_ini = get_loader_ini_dir();
            if let Err(err) = save_value_ext(ext_ini, LOADER_SECTIONS[0], LOADER_KEYS[1], value) {
                error!("{err}");
                ui.display_msg(&err.to_string());
                return !state;
            }
            info!("show_terminal set to {}", state);
            state
        }
    });
    ui.global::<SettingsLogic>().on_set_load_delay({
        let ui_handle = ui.as_weak();
        move |time| {
            let span = info_span!("set_load_delay");
            let _gaurd = span.enter();

            let ui = ui_handle.unwrap();
            ui.global::<MainLogic>().invoke_force_app_focus();
            if let Err(err) = save_value_ext(
                get_loader_ini_dir(),
                LOADER_SECTIONS[0],
                LOADER_KEYS[0],
                &time,
            ) {
                error!("{err}");
                ui.display_msg(&format!("Failed to set load delay\n\n{err}"));
                return;
            }
            info!("load_delay set to {}ms", time);
            ui.global::<SettingsLogic>()
                .set_load_delay(SharedString::from(format!("{time}ms")));
            ui.global::<SettingsLogic>().set_delay_input(SharedString::new());
        }
    });
    ui.global::<SettingsLogic>().on_toggle_all({
        let ui_handle = ui.as_weak();
        move |state| -> bool {
            let span = info_span!("toggle_all");
            let _gaurd = span.enter();

            let ui = ui_handle.unwrap();
            let game_dir = get_or_update_game_dir(None);
            let loader = ModLoader::properties(&game_dir).unwrap_or_else(|err| {
                ui.display_msg(&err.to_string());
                error!("{err}");
                ModLoader::new(!state)
            });
            let files = if loader.disabled() {
                vec![PathBuf::from(LOADER_FILES[0])]
            } else {
                vec![PathBuf::from(LOADER_FILES[1])]
            };
            let mut main_dll = RegMod::new(LOADER_FILES[1], !loader.disabled(), files);
            match toggle_files(&game_dir, !state, &mut main_dll, None) {
                Ok(_) => state,
                Err(err) => {
                    error!("{err}");
                    ui.display_msg(&format!("{err}"));
                    !state
                }
            }
        }
    });
    ui.global::<SettingsLogic>().on_open_game_dir({
        let ui_handle = ui.as_weak();
        move || {
            let span = info_span!("open_game_dir");
            let _gaurd = span.enter();

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
                        ui.display_msg(&format!("{err}"));
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
                let span = info_span!("scan_for_mods");
                let _gaurd = span.enter();
                let game_dir = get_or_update_game_dir(None);
                if let Err(err) = confirm_scan_mods(ui.as_weak(), &game_dir, None, None).await {
                    ui.display_msg(&err.to_string());
                };
            })
            .unwrap();
        }
    });
    ui.global::<MainLogic>().on_add_remove_order({
        let ui_handle = ui.as_weak();
        move |state, key, value| -> i32 {
            let span = info_span!("add_remove_order");
            let _gaurd = span.enter();

            let ui = ui_handle.unwrap();
            let error = 42069_i32;
            let cfg_dir = get_loader_ini_dir();
            let result: i32 = if state { 1 } else { -1 };
            let mut load_order = match ModLoaderCfg::read(cfg_dir) {
                Ok(data) => data,
                Err(err) => {
                    error!("{err}");
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
                        warn!(?key, "Could not find key in {}", LOADER_FILES[2]);
                        return error;
                    }
                    load_orders.remove(&key);
                    None
                }
            };
            if let Err(err) = load_order.update_order_entries(stable_k) {
                error!("{err}");
                ui.display_msg(&format!(
                    "Failed to write to \"mod_loader_config.ini\"\n{err}"
                ));
                return error;
            };
            let model = ui.global::<MainLogic>().get_current_mods();
            let mut selected_mod =
                model.row_data(value as usize).expect("front end gives us valid row");
            selected_mod.order.set = state;
            if !state {
                selected_mod.order.at = 0;
                if selected_mod.dll_files.row_count() != 1 {
                    selected_mod.order.i = -1;
                }
            }
            model.set_row_data(value as usize, selected_mod);
            match load_order.parse_section() {
                Ok(ref order_map) => model.update_order(Some(value), order_map, ui.as_weak()),
                Err(err) => {
                    error!("{err}");
                    ui.display_msg(&err.to_string());
                    return error;
                }
            }
            match state {
                true => info!("Load order set to {}, for {}", value + 1, key),
                false => info!("Load order removed for {}", key),
            }
            result
        }
    });
    ui.global::<MainLogic>().on_modify_order({
        let ui_handle = ui.as_weak();
        move |to_k, from_k, value, row, dll_i| -> i32 {
            let span = info_span!("modify_order");
            let _gaurd = span.enter();

            let ui = ui_handle.unwrap();
            let mut result = 0_i32;
            let error = -1_i32;
            let cfg_dir = get_loader_ini_dir();
            let mut load_order = match ModLoaderCfg::read(cfg_dir) {
                Ok(data) => data,
                Err(err) => {
                    error!("{err}");
                    ui.display_msg(&err.to_string());
                    return error;
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

            if let Err(err) = load_order.update_order_entries(Some(&to_k)) {
                error!("{err}");
                ui.display_msg(&format!(
                    "Failed to write to \"mod_loader_config.ini\"\n{err}"
                ));
                return error;
            };

            if to_k != from_k {
                let model = ui.global::<MainLogic>().get_current_mods();
                let mut selected_mod =
                    model.row_data(row as usize).expect("front end gives us valid row");
                selected_mod.order.i = dll_i;
                if !selected_mod.order.set {
                    selected_mod.order.set = true
                }
                model.set_row_data(row as usize, selected_mod);
                if value != row {
                    match load_order.parse_section() {
                        Ok(ref order_map) => model.update_order(Some(row), order_map, ui.as_weak()),
                        Err(err) => {
                            error!("{err}");
                            ui.display_msg(&err.to_string());
                            return error;
                        }
                    }
                }
            } else if value != row {
                let model = ui.global::<MainLogic>().get_current_mods();
                let mut curr_row =
                    model.row_data(row as usize).expect("front end gives us valid row");
                let mut replace_row =
                    model.row_data(value as usize).expect("front end gives us valid row");
                std::mem::swap(&mut curr_row.order.at, &mut replace_row.order.at);
                model.set_row_data(row as usize, replace_row);
                model.set_row_data(value as usize, curr_row);
                ui.invoke_update_mod_index(value, 1);
                ui.invoke_redraw_checkboxes();
            }
            if !from_k.is_empty() && to_k != from_k {
                info!("Load order removed for {}", from_k);
            }
            info!("Load order set to {}, for {}", value + 1, to_k);
            result
        }
    });
    ui.global::<MainLogic>().on_force_deserialize({
        let ui_handle = ui.as_weak();
        move || {
            let span = info_span!("force_deserialize");
            let _gaurd = span.enter();

            reset_app_state(
                Cfg::default(get_ini_dir()),
                &get_or_update_game_dir(None),
                None,
                ui_handle.clone(),
            );
            info!("Re-loaded all mods after encountered error");
        }
    });

    ui.invoke_focus_app();
    ui.run()
}

trait Sortable {
    fn update_order(
        &self,
        selected_row: Option<i32>,
        order_map: &OrderMap,
        ui_handle: slint::Weak<App>,
    );
}

impl Sortable for ModelRc<DisplayMod> {
    #[instrument(level = "trace", skip_all)]
    fn update_order(
        &self,
        selected_row: Option<i32>,
        order_map: &OrderMap,
        ui_handle: slint::Weak<App>,
    ) {
        let ui = ui_handle.unwrap();
        let selected_key = selected_row.map(|row| {
            self.row_data(row as usize)
                .expect("front end gives us a valid row")
                .name
        });
        let mut unsorted_idx = (0..self.row_count()).collect::<Vec<_>>();
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
            if curr_key.is_some() && {
                new_order = order_map.get(&curr_key.unwrap().to_string());
                new_order
            }
            .is_some()
            {
                let new_order = new_order.unwrap();
                curr_row.order.at = *new_order as i32 + 1;
                if let Some(ref key) = selected_key {
                    if curr_row.name == key {
                        selected_i = *new_order;
                    }
                }
                if unsorted_i == *new_order {
                    self.set_row_data(*new_order, curr_row);
                    unsorted_idx.swap_remove(i);
                    continue;
                }
                if let Some(index) = unsorted_idx.iter().position(|&x| x == *new_order) {
                    let swap_row = self.row_data(*new_order).expect("`ModLoaderCfg.parse_section()` makes sure that `new_order` is always valid");
                    if let Some(ref key) = selected_key {
                        if swap_row.name == key {
                            selected_i = unsorted_i;
                        }
                    }
                    self.set_row_data(*new_order, curr_row);
                    self.set_row_data(unsorted_i, swap_row);
                    unsorted_idx.swap_remove(index);
                    continue;
                }
            }
            if let Some(ref key) = selected_key {
                if curr_row.name == key {
                    selected_i = unsorted_i;
                }
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
        if selected_key.is_some() {
            ui.invoke_update_mod_index(selected_i as i32, 1);
        }
        ui.invoke_redraw_checkboxes();
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

fn get_user_folder(path: &Path, ui_handle: slint::Weak<App>) -> std::io::Result<PathBuf> {
    let ui = ui_handle.unwrap();
    let f_result = match rfd::FileDialog::new()
        .set_directory(path)
        .set_parent(&ui.window().window_handle())
        .pick_folder()
    {
        Some(file) => {
            trace!("User Selected Path: \"{}\"", file.display());
            Ok(file)
        }
        None => new_io_error!(ErrorKind::InvalidInput, "No Path Selected"),
    };
    // workaround for whatever bug in rfd that doesn't interact well with the app when a user
    // performs a secondary action within the file dialog
    let mut size = ui.window().size();
    size.height += 1;
    ui.window().set_size(size);
    size.height -= 1;
    ui.window().set_size(size);
    f_result
}

fn get_user_files(path: &Path, ui_handle: slint::Weak<App>) -> std::io::Result<Vec<PathBuf>> {
    let ui = ui_handle.unwrap();
    let f_result = match rfd::FileDialog::new()
        .set_directory(path)
        .set_parent(&ui.window().window_handle())
        .pick_files()
    {
        Some(files) => match files.len() {
            0 => new_io_error!(ErrorKind::InvalidInput, "No Files Selected"),
            _ => {
                let restricted_files = RESTRICTED_FILES.get().unwrap();
                if files.iter().any(|file| {
                    restricted_files.contains(file.file_name().expect("has valid name"))
                }) {
                    new_io_error!(
                        ErrorKind::InvalidData,
                        "Error: Tried to add a restricted file"
                    )
                } else {
                    trace!("User Selected Files: {files:?}");
                    Ok(files)
                }
            }
        },
        None => {
            new_io_error!(ErrorKind::InvalidInput, "No Files Selected")
        }
    };
    // workaround for whatever bug in rfd that doesn't interact well with the app when a user
    // performs a secondary action within the file dialog
    let mut size = ui.window().size();
    size.height += 1;
    ui.window().set_size(size);
    size.height -= 1;
    ui.window().set_size(size);
    f_result
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
    LOADER_CONFIG_PATH.get_or_init(|| get_or_update_game_dir(None).join(LOADER_FILES[2]))
}

fn get_or_update_game_dir(
    update: Option<PathBuf>,
) -> tokio::sync::RwLockReadGuard<'static, std::path::PathBuf> {
    static GAME_DIR: OnceLock<RwLock<PathBuf>> = OnceLock::new();

    if let Some(path) = update {
        let gd = GAME_DIR.get_or_init(|| RwLock::new(PathBuf::new()));
        let mut gd_lock = gd.blocking_write();
        *gd_lock = path;
    }

    GAME_DIR.get().unwrap().blocking_read()
}

#[inline]
fn populate_restricted_files() -> HashSet<&'static OsStr> {
    LOADER_FILES
        .iter()
        .chain(REQUIRED_GAME_FILES.iter())
        .map(OsStr::new)
        .collect()
}

#[instrument(level = "trace", skip(ui_handle))]
fn open_text_files(ui_handle: slint::Weak<App>, files: Vec<PathBuf>) {
    let ui = ui_handle.unwrap();
    for file in files {
        let file_clone = file.clone();
        let jh =
            std::thread::spawn(move || std::process::Command::new("notepad").arg(&file).spawn());
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
                error!(?err, "Thread panicked!");
                ui.display_msg(&format!("{err:?}"));
            }
        }
    }
}

#[instrument(level = "trace", skip_all, fields(path))]
fn order_data_or_default(ui_handle: slint::Weak<App>, from_path: Option<&Path>) -> OrderMap {
    let ui = ui_handle.unwrap();
    let path = from_path.unwrap_or_else(|| get_loader_ini_dir());
    tracing::Span::current().record("path", path.display().to_string());
    match ModLoaderCfg::read(path) {
        Ok(mut data) => data.parse_section().unwrap_or_else(|err| {
            error!("{err}");
            ui.display_msg(&err.to_string());
            HashMap::new()
        }),
        Err(err) => {
            error!("{err}");
            ui.display_msg(&err.to_string());
            HashMap::new()
        }
    }
}

/// forces all data to be re-read from file, it is fine to pass in a `Cfg::default()` here  
#[instrument(level = "trace", skip_all)]
fn reset_app_state(
    mut cfg: Cfg,
    game_dir: &Path,
    loader_dir: Option<&Path>,
    ui_handle: slint::Weak<App>,
) {
    let ui = ui_handle.unwrap();
    ui.global::<MainLogic>().set_current_subpage(0);
    cfg.update().unwrap_or_else(|err| {
        let dsp_err = "failed to read config data from file";
        error!("{dsp_err} {err}");
        ui.display_msg(dsp_err);
        cfg.empty_contents();
    });
    let order_data = order_data_or_default(ui.as_weak(), loader_dir);
    deserialize_collected_mods(
        &cfg.collect_mods(game_dir, Some(&order_data), false),
        ui.as_weak(),
    );
    info!("reloaded state from file");
}

type DeserializedFileData = (
    ModelRc<StandardListViewItem>,
    ModelRc<SharedString>,
    ModelRc<SharedString>,
);
/// deserializes `SplitFiles` to `ModelRc<T>` where `T` is the type the front end expects  
/// output is in the following order (`files`, `dll_files`, `config_files`)
fn deserialize_split_files(split_files: &SplitFiles) -> DeserializedFileData {
    let files: Rc<VecModel<StandardListViewItem>> = Default::default();
    let dll_files: Rc<VecModel<SharedString>> = Default::default();
    let config_files: Rc<VecModel<SharedString>> = Default::default();
    if !split_files.dll.is_empty() {
        files.extend(
            split_files
                .dll
                .iter()
                .map(|f| SharedString::from(omit_off_state(&f.to_string_lossy())).into()),
        );
        dll_files.extend(split_files.dll.iter().map(|f| {
            SharedString::from(omit_off_state(
                &f.file_name().expect("file validated").to_string_lossy(),
            ))
        }));
    };
    if !split_files.config.is_empty() {
        files.extend(
            split_files
                .config
                .iter()
                .map(|f| SharedString::from(f.to_string_lossy().to_string()).into()),
        );
        config_files.extend(
            split_files
                .config
                .iter()
                .map(|f| SharedString::from(f.to_string_lossy().to_string())),
        );
    };
    if !split_files.other.is_empty() {
        files.extend(
            split_files
                .other
                .iter()
                .map(|f| SharedString::from(f.to_string_lossy().to_string()).into()),
        );
    }
    (
        ModelRc::from(files),
        ModelRc::from(dll_files),
        ModelRc::from(config_files),
    )
}

fn deserialize_mod(mod_data: &RegMod) -> DisplayMod {
    let (files, dll_files, config_files) = deserialize_split_files(&mod_data.files);
    let name = mod_data.name.replace('_', " ");
    DisplayMod {
        displayname: SharedString::from(if mod_data.name.chars().count() > 20 {
            name.chars().take(17).chain("...".chars()).collect()
        } else {
            name.clone()
        }),
        name: SharedString::from(name),
        enabled: mod_data.state,
        files,
        config_files,
        dll_files,
        order: LoadOrder {
            at: if !mod_data.order.set {
                0
            } else {
                mod_data.order.at as i32 + 1
            },
            i: if !mod_data.order.set && mod_data.files.dll.len() != 1 {
                -1
            } else {
                mod_data.order.i as i32
            },
            set: mod_data.order.set,
        },
    }
}

#[instrument(level = "trace", skip_all)]
fn deserialize_collected_mods(data: &CollectedMods, ui_handle: slint::Weak<App>) {
    let ui = ui_handle.unwrap();
    if let Some(ref warning) = data.warnings {
        warn!("{warning}");
        ui.display_msg(&warning.to_string());
    }

    let display_mods: Rc<VecModel<DisplayMod>> = Default::default();
    data.mods
        .iter()
        .for_each(|mod_data| display_mods.push(deserialize_mod(mod_data)));

    ui.global::<MainLogic>().set_current_mods(ModelRc::from(display_mods));
    ui.global::<MainLogic>()
        .set_orders_set(data.mods.order_count() as i32);
    trace!("deserialized mods");
}

#[instrument(level = "trace", skip_all)]
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
    let data = InstallData::new(mod_name, files, game_dir)?;
    add_dir_to_install_data(data, ui_handle).await
}

#[instrument(level = "trace", skip_all)]
async fn install_new_files_to_mod(
    mod_data: &RegMod,
    files: Vec<PathBuf>,
    game_dir: &Path,
    ui_handle: slint::Weak<App>,
) -> std::io::Result<Vec<PathBuf>> {
    let ui = ui_handle.unwrap();
    ui.display_confirm(
        "Selected files are not installed? Would you like to try and install them?",
        true,
    );
    if receive_msg().await != Message::Confirm {
        return new_io_error!(
            ErrorKind::ConnectionAborted,
            "Did not select to install files"
        );
    };
    let data = InstallData::amend(mod_data, files, game_dir)?;
    confirm_install(data, ui_handle).await
}

#[instrument(level = "trace", skip_all)]
async fn add_dir_to_install_data(
    mut install_files: InstallData,
    ui_handle: slint::Weak<App>,
) -> std::io::Result<Vec<PathBuf>> {
    let ui = ui_handle.unwrap();
    ui.display_confirm(&format!(
        "Current Files to install:\n{}\n\nWould you like to add a directory eg. Folder containing a config file?", 
        install_files.display_paths), true);
    let mut result = Vec::with_capacity(1);
    match receive_msg().await {
        Message::Confirm => match get_user_folder(&install_files.parent_dir, ui.as_weak()) {
            Ok(path) => {
                install_files
                    .update_fields_with_new_dir(&path, utils::installer::DisplayItems::Limit(9))
                    .await
                    .unwrap_or_else(|err| {
                        error!("{err}");
                        result.push(err);
                    });
            }
            Err(err) => result.push(err),
        },
        Message::Deny => (),
        Message::Esc => return new_io_error!(ErrorKind::ConnectionAborted, "Mod install canceled"),
    }
    match result.is_empty() {
        false => {
            let err = &result[0];
            if err.kind() == ErrorKind::InvalidInput {
                ui.display_msg(&format!("{err}"));
                let _ = receive_msg().await;
                let reselect_dir =
                    Box::pin(async { add_dir_to_install_data(install_files, ui_handle).await });
                reselect_dir.await
            } else {
                error!("{err}");
                new_io_error!(ErrorKind::Other, format!("Error: Could not Install, {err}"))
            }
        }
        true => confirm_install(install_files, ui_handle).await,
    }
}

#[instrument(level = "trace", skip_all)]
async fn confirm_install(
    install_files: InstallData,
    ui_handle: slint::Weak<App>,
) -> std::io::Result<Vec<PathBuf>> {
    let ui = ui_handle.unwrap();
    ui.display_confirm(
        &format!(
            "Confirm install of mod: {}\n\nSelected files:\n{}\n\nInstall at:\n{}",
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
    if zip
        .iter()
        .any(|(_, to_path)| !matches!(to_path.try_exists(), Ok(false)))
    {
        return new_io_error!(
            ErrorKind::InvalidInput,
            format!(
                "Could not install: {}\".\nA selected file is already installed",
                install_files.name
            )
        );
    };
    let parents = zip
        .iter()
        .map(|(_, to_path)| parent_or_err(to_path))
        .collect::<std::io::Result<Vec<&Path>>>()?;
    parents.iter().try_for_each(std::fs::create_dir_all)?;
    zip.iter()
        .try_for_each(|(from_path, to_path)| std::fs::copy(from_path, to_path).map(|_| ()))?;
    let success = format!("Installed mod: {}", &install_files.name);
    info!("{success}");
    ui.display_msg(&success);
    Ok(zip.iter().map(|(_, to_path)| to_path.to_path_buf()).collect())
}

#[instrument(level = "trace", skip_all, fields(mod_name = reg_mod.name))]
async fn confirm_remove_mod(
    ui_handle: slint::Weak<App>,
    game_dir: &Path,
    loader_dir: &Path,
    reg_mod: &RegMod,
) -> std::io::Result<()> {
    let ui = ui_handle.unwrap();
    let install_dir = match reg_mod
        .files
        .file_refs()
        .iter()
        .min_by_key(|file| file.ancestors().count())
    {
        Some(path) => game_dir.join(parent_or_err(path)?),
        None => return new_io_error!(ErrorKind::InvalidData, "Failed to create an install_dir"),
    };
    ui.display_confirm(
        "Do you want to remove mod files from the game directory?",
        true,
    );
    if receive_msg().await != Message::Confirm {
        return new_io_error!(
            ErrorKind::ConnectionAborted,
            format!(
                "Files registered with: {}, are still installed at \"{}\"",
                DisplayName(&reg_mod.name),
                install_dir.display()
            )
        );
    };
    ui.display_confirm(
        "This is a distructive action. Are you sure you want to continue?",
        false,
    );
    if receive_msg().await != Message::Confirm {
        return new_io_error!(
            ErrorKind::ConnectionAborted,
            format!(
                "Files registered with: {}, are still installed at \"{}\"",
                DisplayName(&reg_mod.name),
                install_dir.display()
            )
        );
    };
    remove_mod_files(game_dir, loader_dir, reg_mod)
}

#[instrument(level = "trace", skip_all)]
async fn confirm_scan_mods(
    ui_handle: slint::Weak<App>,
    game_dir: &Path,
    ini: Option<&Cfg>,
    order_map: Option<&OrderMap>,
) -> std::io::Result<()> {
    let ui = ui_handle.unwrap();

    ui.display_confirm(
        "Would you like to attempt to auto-import already installed mods to Elden Mod Loader GUI?",
        true,
    );
    if receive_msg().await != Message::Confirm {
        return Ok(());
    };

    let mut _new_ini = None;
    let ini = match ini {
        Some(data) => data,
        None => {
            _new_ini = Some(Cfg::read(get_ini_dir())?);
            _new_ini.as_ref().unwrap()
        }
    };
    let loader_dir = get_loader_ini_dir();
    let mut _new_map = None;
    let order_map = order_map.unwrap_or_else(|| {
        _new_map = Some(order_data_or_default(ui.as_weak(), Some(loader_dir)));
        _new_map.as_ref().unwrap()
    });

    let mut old_mods: Vec<RegMod>;
    if !ini.mods_is_empty() {
        ui.display_confirm("Warning: This action will reset current registered mods, are you sure you want to continue?", true);
        if receive_msg().await != Message::Confirm {
            return Ok(());
        };
        old_mods = {
            let data = ini.collect_mods(game_dir, Some(order_map), false);
            if let Some(warning) = data.warnings {
                ui.display_msg(&warning.to_string());
            }
            data.mods
        };
        let dark_mode = ui.global::<SettingsLogic>().get_dark_mode();
        let save_log = ini.get_save_log().unwrap_or(true);

        std::fs::remove_file(ini.path())?;
        new_cfg(ini.path())?;
        if dark_mode != DEFAULT_INI_VALUES[0] {
            save_bool(ini.path(), INI_SECTIONS[0], INI_KEYS[0], dark_mode)?;
        }
        if save_log != DEFAULT_INI_VALUES[1] {
            save_bool(ini.path(), INI_SECTIONS[0], INI_KEYS[1], save_log)?;
        }
        save_path(ini.path(), INI_SECTIONS[1], INI_KEYS[2], game_dir)?;
    } else {
        old_mods = Vec::new();
    }
    let new_mods: CollectedMods;
    match scan_for_mods(game_dir, ini.path()) {
        Ok(len) => {
            let new_ini = Cfg::read(ini.path())?;
            ui.global::<MainLogic>().set_current_subpage(0);
            let order_data = order_data_or_default(ui.as_weak(), Some(loader_dir));
            new_mods = new_ini.collect_mods(game_dir, Some(&order_data), false);
            deserialize_collected_mods(&new_mods, ui.as_weak());
            ui.display_msg(&format!("Found {len} mod(s)"));
        }
        Err(err) => {
            ui.display_msg(&format!("{err}"));
            new_mods = CollectedMods {
                mods: Vec::new(),
                warnings: None,
            };
        }
    };
    if let Some(warning) = new_mods.warnings {
        warn!(%warning);
        ui.display_msg(&warning.to_string());
    }
    if !old_mods.is_empty() {
        let all_new_files = new_mods
            .mods
            .iter()
            .flat_map(|m| m.files.file_refs())
            .collect::<HashSet<_>>();
        old_mods.retain(|m| m.files.dll.iter().any(|f| !all_new_files.contains(f.as_path())));
        if old_mods.is_empty() {
            return Ok(());
        }

        // unsure if we want to remove order data, currently on mod removal we do not delete order data,
        // we only delete order data on mod uninstallation
        old_mods.iter().try_for_each(|m| {
            if m.order.set && !all_new_files.contains(m.files.dll[m.order.i].as_path()) {
                remove_order_entry(m, loader_dir)
            } else {
                Ok(())
            }
        })?;

        old_mods
            .iter_mut()
            .for_each(|m| m.files.dll.retain(|f| !all_new_files.contains(f.as_path())));
        old_mods.retain(|m| !m.files.dll.is_empty());
        if old_mods.is_empty() {
            return Ok(());
        }
        old_mods.retain(|m| m.files.dll.iter().any(FileData::is_disabled));
        if old_mods.is_empty() {
            return Ok(());
        }

        old_mods
            .iter_mut()
            .try_for_each(|m| toggle_files(game_dir, true, m, None).map(|_| ()))?;
    }
    Ok(())
}

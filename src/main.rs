#![cfg(target_os = "windows")]
// Setting windows_subsystem will hide console | cant read logs if console is hidden
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use elden_mod_loader_gui::{
    utils::{
        display::*,
        ini::{
            common::*,
            mod_loader::{ModLoader, OrdMetaData, RegModsExt},
            parser::{CollectedMods, RegMod, Setup, SplitFiles},
            writer::*,
        },
        installer::{remove_mod_files, scan_for_mods, InstallData},
        subscriber::init_subscriber,
    },
    *,
};
use i_slint_backend_winit::WinitWindowAccessor;
use slint::{ComponentHandle, Model, ModelRc, SharedString, StandardListViewItem, VecModel};
use std::{
    collections::{HashMap, HashSet, VecDeque},
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
static UNKNOWN_ORDER_KEYS: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();
static RECEIVER: OnceLock<RwLock<UnboundedReceiver<MessageData>>> = OnceLock::new();

const ERROR_VAL: i32 = 42069;
const OK_VAL: i32 = 0;

fn main() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        error!(name: "PANIC", "{}", format_panic_info(info));
        prev(info);
    }));

    let mut dsp_msgs = Vec::new();
    let _guard = init_subscriber().unwrap_or_else(|err| {
        dsp_msgs.push(err.to_string());
        None
    });

    slint::platform::set_platform(Box::new(
        i_slint_backend_winit::Backend::new().expect("This app is being run on windows"),
    ))
    .expect("This app uses the winit backend");

    let ui = App::new().unwrap();
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
        let _guard = span.enter();

        let current_ini = get_ini_dir();
        let first_startup: bool;
        let ini = match current_ini.is_setup(&INI_SECTIONS) {
            Ok(ini_data) => {
                first_startup = false;
                Some(ini_data)
            }
            Err(err) => {
                first_startup = matches!(
                    err.kind(),
                    ErrorKind::NotFound | ErrorKind::PermissionDenied
                );
                if err.kind() == ErrorKind::NotFound {
                    info!("{err}");
                } else {
                    error!(err_code = 1, "{err}");
                    dsp_msgs.push(err.to_string())
                }
                None
            }
        };
        let mut ini = if let Some(ini_data) = ini {
            let mut ini: Cfg = Config::from(ini_data, current_ini);
            if let Err(messages) = ini.validate_entries() {
                dsp_msgs.extend(messages);
                ini.write_to_file()
                    .unwrap_or_else(|err| panic!("{err}, while writing contents to: {INI_NAME}"));
            };
            ini
        } else {
            new_cfg(current_ini)
                .map(|ini| Config::from(ini, current_ini))
                .unwrap_or_else(|err| {
                    // io::write error
                    error!(err_code = 2, "{err}");
                    dsp_msgs.push(err.to_string());
                    Cfg::default(current_ini)
                })
        };

        let game_verified: bool;
        let mod_loader: ModLoader;
        let mut mod_loader_cfg: ModLoaderCfg;
        let mut reg_mods = None;
        let mut order_data = None;
        let mut ord_meta_data = None;
        let game_dir = match ini.attempt_locate_game() {
            Ok(PathResult::Full(path)) => {
                mod_loader = ModLoader::properties(&path).unwrap_or_else(|err| {
                    error!(err_code = 3, "{err}");
                    dsp_msgs.push(err.to_string());
                    ModLoader::default()
                });
                if mod_loader.installed() {
                    info!(dll_hook = %DisplayState(!mod_loader.disabled()), "elden_mod_loader files found");
                    mod_loader_cfg = ModLoaderCfg::read(mod_loader.path()).unwrap_or_else(|err| {
                        error!(err_code = 4, "{err}");
                        dsp_msgs.push(err.to_string());
                        ModLoaderCfg::default(mod_loader.path())
                    });
                    let (dlls, order_count, update_loader) =
                        ini.dll_set_order_count(mod_loader_cfg.mut_section());
                    if update_loader {
                        mod_loader_cfg.write_to_file().unwrap_or_else(|err| {
                            error!(err_code = 5, "{err}");
                            dsp_msgs.push(err.to_string());
                        });
                    }
                    if let Err(key_err) = mod_loader_cfg.verify_keys(&dlls, order_count) {
                        match key_err.err.kind() {
                            ErrorKind::Unsupported => {
                                ini.update().unwrap_or_else(|err| {
                                    error!(err_code = 6, "{err}");
                                });
                                ord_meta_data = key_err.update_ord_data;
                                warn!("{}", key_err.err);
                            }
                            ErrorKind::Other => info!("{}", key_err.err),
                            _ => error!(err_code = 7, "{}", key_err.err),
                        }
                        if let Some(unknown_keys) = key_err.unknown_keys {
                            UNKNOWN_ORDER_KEYS
                                .set(RwLock::new(unknown_keys))
                                .expect("only initial set");
                        }
                        dsp_msgs.push(key_err.err.to_string());
                    }
                    order_data = mod_loader_cfg
                        .parse_section(&get_unknown_orders())
                        .map(Some)
                        .unwrap_or_else(|err| {
                            error!(err_code = 8, "{err}");
                            dsp_msgs.push(err.to_string());
                            None
                        });
                } else {
                    mod_loader_cfg = ModLoaderCfg::default(mod_loader.path());
                }
                info!(
                    "{}",
                    DisplayAntiCheatFound(mod_loader.anti_cheat_toggle_installed())
                );
                reg_mods = {
                    let mut collection = ini.collect_mods(&path, order_data.as_ref(), false);
                    if collection.mods.len() != ini.mods_registered() {
                        ini.update().unwrap_or_else(|err| {
                            error!(err_code = 9, "{err}");
                        });
                    }
                    if let Some(warning) = collection.warnings.take() {
                        dsp_msgs.push(warning.to_string());
                    }
                    info!(
                        "Found {} mod(s) registered in: {}",
                        collection.mods.len(),
                        INI_NAME
                    );
                    if ord_meta_data.is_none() {
                        ord_meta_data = Some(OrdMetaData::with_ord(collection.mods.max_order()));
                    }
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
                error!(err_code = 10, "{err}");
                dsp_msgs.push(err.to_string());
                mod_loader_cfg = ModLoaderCfg::empty();
                mod_loader = ModLoader::default();
                game_verified = false;
                None
            }
        };

        ui.global::<SettingsLogic>()
            .set_dark_mode(ini.get_dark_mode().unwrap_or_else(|err| {
                // parse error ErrorKind::InvalidData
                error!(err_code = 11, "{err}");
                dsp_msgs.push(err.to_string());
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
        if let Some(meta_data) = ord_meta_data {
            ui.global::<MainLogic>()
                .set_max_order(MaxOrder::from(meta_data.max_order));
            if let Some(ref vals) = meta_data.missing_vals {
                let msg = DisplayMissingOrd(vals).to_string();
                info!("{msg}");
                dsp_msgs.push(msg);
            }
        }
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
                    error!(err_code = 12, "{err}");
                    dsp_msgs.push(err.to_string());
                    DEFAULT_LOADER_VALUES[0].parse().unwrap()
                });
                let show_terminal = mod_loader_cfg.get_show_terminal().unwrap_or_else(|err| {
                    // parse error ErrorKind::InvalidData
                    error!(err_code = 13, "{err}");
                    dsp_msgs.push(err.to_string());
                    false
                });

                ui.global::<SettingsLogic>()
                    .set_load_delay(SharedString::from(format!("{delay}ms")));
                ui.global::<SettingsLogic>().set_show_terminal(show_terminal);

                if mod_loader.anti_cheat_enabled() {
                    dsp_msgs.push(DisplayAntiCheatMsg.to_string());
                }
            }
        }
        // we need to wait for slint event loop to start `ui.run()` before making calls to `ui.display_msg()`
        // otherwise calculations for the positon of display_msg_popup are not correct
        let ui_handle = ui.as_weak();
        let span_clone = span.clone();
        slint::invoke_from_event_loop(move || {
            slint::Timer::single_shot(std::time::Duration::from_millis(200), move || {
                slint::spawn_local(async move {
                    let _guard = span_clone.enter();
                    let ui = ui_handle.unwrap();
                    if !dsp_msgs.is_empty() {
                        for msg in dsp_msgs {
                            ui.display_msg(&msg);
                            let _ = receive_msg().await;
                        }
                    }
                    let mut disp_msg = if first_startup {
                        String::from(
                            "Welcome to Elden Mod Loader GUI!\n\
                            Thanks for downloading, please report any bugs"
                        )
                    } else {
                        String::new()
                    };
                    if first_startup && game_verified {
                        disp_msg.push_str("\n\nGame Files Found!")
                    }
                    // display info level to user
                    if !disp_msg.is_empty() {
                        ui.display_msg(&std::mem::take(&mut disp_msg));
                        let _ = receive_msg().await;
                    }
                    if !game_verified {
                        disp_msg = String::from("Could not locate Elden Ring\nPlease Select the install directory for Elden Ring")
                    } else if !mod_loader.installed() {
                        disp_msg = format!(
                            "{TECHIE_W_MSG}\n\n\
                            Please install files to: '{}', and relaunch Elden Mod Loader GUI", game_dir.as_ref().expect("game_verified").display()
                        )
                    }
                    if game_verified && !mod_loader.anti_cheat_toggle_installed() {
                        let anti_cheat_msg = format!(
                            "'{ANTI_CHEAT_EXE}' not found, do not forget to disable Easy-AntiCheat before running Elden Ring with mods installed"
                        );
                        if disp_msg.is_empty() {
                            disp_msg = anti_cheat_msg
                        } else {
                            disp_msg.push_str(&format!("\n\n{anti_cheat_msg}"))
                        }
                    }
                    // display warn level to user
                    if !disp_msg.is_empty() {
                        ui.display_msg(&std::mem::take(&mut disp_msg));
                        let _ = receive_msg().await;
                    }
                    if first_startup && game_verified && mod_loader.installed() {
                        ui.display_msg(TUTORIAL_MSG);
                        let _ = receive_msg().await;
                    }
                    if (game_verified && mod_loader.installed()) && (first_startup || ini.mods_is_empty()) {
                        if let Err(err) = confirm_scan_mods(
                            ui.as_weak(),
                            game_dir.as_ref().expect("game_verified"),
                            Some(&ini),
                            order_data.as_ref()
                        ).await {
                            ui.display_and_log_err(err);
                        };
                    }
                }).unwrap();
            });
        }).unwrap();
    }

    ui.global::<MainLogic>().on_select_mod_files({
        let ui_handle = ui.as_weak();
        move |mod_name| {
            let span = info_span!("add_mod");
            let _guard = span.enter();

            let ui = ui_handle.unwrap();
            let ini_dir = get_ini_dir();
            let game_dir = get_or_update_game_dir(None);
            let mut ini = match Cfg::read(ini_dir) {
                Ok(ini_data) => ini_data,
                Err(err) => {
                    ui.display_and_log_err(err);
                    return;
                }
            };
            let format_key = mod_name.trim().replace(' ', "_");
            if ini.keys().contains(&format_key.to_lowercase()) {
                ui.display_msg(&format!(
                    "There is already a registered mod with the name\n\"{mod_name}\""
                ));
                ui.global::<MainLogic>()
                    .set_line_edit_text(SharedString::new());
                return;
            }
            let span_clone = span.clone();
            slint::spawn_local(async move {
                let _guard = span_clone.enter();
                let mut file_paths = match get_user_files(&game_dir, ui.window()) {
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
                        if file_paths.len() != err.err_paths_long.len() {
                            error!("Encountered {} StripPrefixError on input files", err.err_paths_long.len());
                            ui.display_msg(
                                &format!(
                                    "Some selected files are already installed\n\nSelected Files Installed: {}\nSelected Files not installed: {}",
                                    err.ok_paths_short.len(),
                                    err.err_paths_long.len()
                                ));
                            return;
                        }
                        match install_new_mod(&mod_name, file_paths, &game_dir, ui.as_weak()).await {
                            Ok(installed_files) => {
                                file_paths = installed_files;
                                match shorten_paths(&file_paths, &game_dir) {
                                    Ok(installed_and_shortend) => installed_and_shortend,
                                    Err(err) => {
                                        let err_string = format!("New mod installed but ran into StripPrefixError on {}", DisplayVec(&err.err_paths_long));
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
                    ui.display_and_log_err(err);
                    ModLoaderCfg::default(loader_dir)
                });
                let mut unknown_orders = get_mut_unknown_orders();
                let order_data = loader_cfg.parse_section(&unknown_orders).unwrap_or_else(|err| {
                    ui.display_and_log_err(err);
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
                    ui.display_and_log_err(err);
                    return;
                };
                for f in new_mod.files.dll.iter() {
                    let Some(f_name) = f.file_name().and_then(|o| o.to_str()).map(omit_off_state) else {
                        error!("Failed to get file name for: {}", f.display());
                        continue;
                    };
                    unknown_orders.remove(f_name);
                }
                ui.global::<MainLogic>().set_line_edit_text(SharedString::new());
                ini.update().unwrap_or_else(|err| {
                    ui.display_and_log_err(err);
                    ini = Cfg::default(ini_dir);
                });

                let model = ui.global::<MainLogic>().get_current_mods();
                let mut_model = model.as_any().downcast_ref::<VecModel<DisplayMod>>().expect("we set this type earlier");
                mut_model.push(deserialize_mod(&new_mod));
                if new_mod.order.set {
                    let ord_meta_data = loader_cfg.update_order_entries(None, &unknown_orders);
                    ui.global::<MainLogic>().set_max_order(MaxOrder::from(ord_meta_data.max_order));
                    model.update_order(None, &order_data, &unknown_orders, ui.as_weak());
                }
                info!(
                    files = new_mod.files.len(),
                    state = %DisplayState(new_mod.state),
                    order = %new_mod.order,
                    "{mod_name} added with"
                );
            }).unwrap();
        }
    });
    ui.global::<SettingsLogic>().on_select_game_dir({
        let ui_handle = ui.as_weak();
        move || {
            let span = info_span!("select_game_path");
            let _guard = span.enter();

            let ui = ui_handle.unwrap();
            let ini = match Cfg::read(get_ini_dir()) {
                Ok(ini_data) => ini_data,
                Err(err) => {
                    ui.display_and_log_err(err);
                    return;
                }
            };
            let game_dir = get_or_update_game_dir(None);
            let path_result = get_user_folder(&game_dir, ui.window());
            drop(game_dir);

            let path = match path_result {
                Ok(path) => path,
                Err(err) => {
                    ui.display_msg(&err.to_string());
                    return;
                }
            };
            let try_path: PathBuf = match does_dir_contain(&path, Operation::All, &["Game"]) {
                Ok(OperationResult::Bool(true)) => {
                    PathBuf::from(&format!("{}\\Game", path.display()))
                }
                Ok(OperationResult::Bool(false)) => path,
                Err(err) => {
                    ui.display_and_log_err(err);
                    return;
                }
                _ => unreachable!(),
            };
            let not_found = match files_not_found(&try_path, &REQUIRED_GAME_FILES) {
                Ok(files) => files,
                Err(err) => {
                    match err.kind() {
                        ErrorKind::NotFound => warn!("{err}"),
                        _ => error!("{err}"),
                    }
                    ui.display_msg(&err.to_string());
                    return;
                }
            };
            if !not_found.is_empty() {
                error!(
                    "Required game files not found in: '{}', files missing: {}",
                    try_path.display(),
                    DisplayVec(&not_found)
                );
                ui.display_msg(&format!(
                    "Could not find Elden Ring in:\n\"{}\"",
                    try_path.display()
                ));
                return;
            }
            if let Err(err) = save_path(ini.path(), INI_SECTIONS[1], INI_KEYS[2], &try_path) {
                error!("Failed to save directory. {err}");
                ui.display_msg(&err.to_string());
                return;
            };

            let span_clone = span.clone();
            slint::spawn_local(async move {
                let _guard = span_clone.enter();
                let mod_loader = ModLoader::properties(&try_path).unwrap_or_default();
                ui.global::<SettingsLogic>()
                    .set_game_path(try_path.to_string_lossy().to_string().into());
                ui.global::<MainLogic>().set_game_path_valid(true);
                ui.global::<MainLogic>().set_current_subpage(0);
                ui.global::<SettingsLogic>()
                    .set_loader_installed(mod_loader.installed());
                ui.global::<SettingsLogic>()
                    .set_loader_disabled(mod_loader.disabled());
                if mod_loader.installed() {
                    ui.display_msg(&format!(
                        "Game Files Found!\n\
                        {TUTORIAL_MSG}"
                    ));
                    let _ = receive_msg().await;
                    if ini.mods_is_empty() {
                        if let Err(err) =
                            confirm_scan_mods(ui.as_weak(), &try_path, Some(&ini), None).await
                        {
                            error!("{err}");
                            ui.display_msg(&err.to_string());
                        };
                    }
                } else {
                    ui.display_msg(&format!(
                        "Game Files Found!\n\n\
                        {TECHIE_W_MSG}"
                    ))
                }
                let _ = get_or_update_game_dir(Some(try_path));
            })
            .unwrap();
        }
    });
    ui.global::<MainLogic>().on_toggle_mod({
        let ui_handle = ui.as_weak();
        move |key, state| -> bool {
            let span = info_span!("toggle_mod");
            let _guard = span.enter();

            let ui = ui_handle.unwrap();
            let ini_dir = get_ini_dir();
            let mut ini = match Cfg::read(ini_dir) {
                Ok(ini_data) => ini_data,
                Err(err) => {
                    ui.display_and_log_err(err);
                    return !state;
                }
            };
            let game_dir = get_or_update_game_dir(None);
            match ini.get_mod(&key, &game_dir, None) {
                Ok(ref mut reg_mod) => {
                    if reg_mod.files.dll.is_empty() {
                        info!(
                            "Can not toggle: {}, mod has no .dll files",
                            DisplayName(&reg_mod.name)
                        );
                        ui.display_msg(&format!(
                            "To toggle: {}, please add a .dll file",
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
                    ui.display_and_log_err(err);
                }
            }
            reset_app_state(&mut ini, &game_dir, None, None, ui.as_weak());
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
            let _guard = span.enter();

            let ui = ui_handle.unwrap();
            let ini_dir = get_ini_dir();
            let game_dir = get_or_update_game_dir(None);
            let mut ini = match Cfg::read(ini_dir) {
                Ok(ini_data) => ini_data,
                Err(err) => {
                    ui.display_and_log_err(err);
                    return;
                }
            };
            let span_clone = span.clone();
            slint::spawn_local(async move {
                let _guard = span_clone.enter();
                let mut file_paths = match get_user_files(&game_dir, ui.window()) {
                    Ok(paths) => paths,
                    Err(err) => {
                        if err.kind() != ErrorKind::InvalidInput {
                            error!("{err}");
                        }
                        ui.display_msg(&err.to_string());
                        return;
                    }
                };
                let mut loader_cfg = ModLoaderCfg::read(get_loader_ini_dir()).unwrap_or_else(|err| {
                    warn!("{err}");
                    ui.display_msg(&err.to_string());
                    ModLoaderCfg::empty()
                });
                let mut unknown_orders = get_mut_unknown_orders();
                let order_map = loader_cfg.parse_section(&unknown_orders).unwrap_or_else(|err| {
                        error!("{err}");
                        ui.display_msg(&err.to_string());
                        loader_cfg.parse_into_map()
                });
                let model = ui.global::<MainLogic>().get_current_mods();
                let mut display_mod = model.row_data(row as usize).expect("front end gives us valid row");
                let mut found_mod = match ini.get_mod(&display_mod.name, &game_dir, Some(&order_map)) {
                    Ok(reg_mod) => reg_mod,
                    Err(err) => {
                        error!("{err}");
                        ui.display_msg(&err.to_string());
                        reset_app_state(&mut ini, &game_dir, None, Some(&unknown_orders), ui.as_weak());
                        return;
                    }
                };
                let files = match shorten_paths(&file_paths, &game_dir) {
                    Ok(files) => files,
                    Err(err) => {
                        if file_paths.len() != err.err_paths_long.len() {
                            error!(files = ?err.err_paths_long, "Encountered {} StripPrefixError(s) on input", err.err_paths_long.len());
                            ui.display_msg(
                                &format!(
                                    "Some selected files are already installed\n\nSelected Files Installed: {}\nSelected Files not installed: {}",
                                    err.ok_paths_short.len(),
                                    err.err_paths_long.len()
                                ));
                            return;
                        }
                        match install_new_files_to_mod(&found_mod, file_paths, &game_dir, ui.as_weak()).await {
                            Ok(installed_files) => {
                                file_paths = installed_files;
                                match shorten_paths(&file_paths, &game_dir) {
                                    Ok(installed_and_shortend) => installed_and_shortend,
                                    Err(err) => {
                                        let err_string = format!("Files installed but ran into StripPrefixError on {}", DisplayVec(&err.err_paths_long));
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
                    ui.display_and_log_err(err);
                    return;
                };
                if let Err(err) = found_mod.verify_state(&game_dir, ini.path()) {
                    ui.display_msg(&err.to_string());
                    let _ = found_mod.remove_from_file(ini.path());
                    let err_str = format!("Failed to verify state, mod was removed {err}");
                    error!("{err_str}");
                    ui.display_msg(&err_str);
                    reset_app_state(&mut ini, &game_dir, None, Some(&unknown_orders), ui.as_weak());
                    return;
                };
                let new_dlls_with_set_order = files.iter().filter_map(|f| {
                    let f_str = f.to_string_lossy();
                    let f_data = FileData::from(file_name_from_str(&f_str));
                    if f_data.extension != ".dll" {
                        return None;
                    }
                    let f_name = f_data.omit_off_state();
                    if unknown_orders.remove(&f_name) {
                        return Some((f_name, *f));
                    }
                    None
                }).collect::<Vec<_>>();
                let dll_added_with_set_order = !new_dlls_with_set_order.is_empty();
                let mut update_order = false;
                let (files, dll_files, config_files) = deserialize_split_files(&found_mod.files);
                display_mod.files = files;
                display_mod.dll_files = dll_files;
                display_mod.config_files = config_files;
                if !found_mod.order.set {
                    if dll_added_with_set_order {
                        let Some(index) = found_mod.files.dll.iter().position(|f| f == new_dlls_with_set_order[0].1) else {
                            let err = format!("File: {}, not correctly added to: {}", new_dlls_with_set_order[0].1.display(), display_mod.name);
                            error!("{err}");
                            ui.display_msg(&err);
                            reset_app_state(&mut ini, &game_dir, Some(loader_cfg.path()), Some(&unknown_orders), ui.as_weak());
                            return;
                        };
                        display_mod.order.set = true;
                        display_mod.order.i = index as i32;
                        display_mod.order.at = *order_map.get(&new_dlls_with_set_order[0].0).expect("entry was previously found as unknown") as i32;
                        update_order = true;
                    } else {
                        match found_mod.files.dll.len() {
                            0 => (),
                            1 => display_mod.order.i = 0,
                            2.. => display_mod.order.i = -1,
                        }
                    }
                } else if dll_added_with_set_order {
                    new_dlls_with_set_order.iter().for_each(|f| {
                        loader_cfg.mut_section().remove(&f.0);
                    });
                    loader_cfg.write_to_file().unwrap_or_else(|err| {
                        error!("{err}");
                        ui.display_msg(&err.to_string());
                    });
                }
                model.set_row_data(row as usize, display_mod);
                if dll_added_with_set_order {
                    let ord_meta_data = loader_cfg.update_order_entries(None, &unknown_orders);
                    ui.global::<MainLogic>().set_max_order(MaxOrder::from(ord_meta_data.max_order));
                }
                if update_order {
                    model.update_order(Some(row), &order_map, &unknown_orders, ui.as_weak());
                }
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
                let _guard = span.enter();
                let ui = handle_clone.unwrap();
                ui.display_confirm(&format!("Are you sure you want to de-register: {key}?"), Buttons::OkCancel);
                if receive_msg().await != Message::Confirm {
                    return
                }
                let ini_dir = get_ini_dir();
                let mut ini = match Cfg::read(ini_dir) {
                    Ok(ini_data) => ini_data,
                    Err(err) => {
                        error!("{err}");
                        ui.display_msg(&err.to_string());
                        return;
                    }
                };
                let loader_dir = get_loader_ini_dir();
                let mut messages = Vec::with_capacity(5);
                let mut unknown_orders = get_mut_unknown_orders();
                let mut loader = match ModLoaderCfg::read(loader_dir) {
                    Ok(data) => data,
                    Err(err) => {
                        error!("{err}");
                        ui.display_msg(&err.to_string());
                        return;
                    }
                };
                let mut order_map = loader.parse_section(&unknown_orders).unwrap_or_else(|err| {
                    error!("{err}");
                    messages.push(err.to_string());
                    loader.parse_into_map()
                });
                let game_dir = get_or_update_game_dir(None);
                let reset_app_state_hook = |err: std::io::Error, mut ini: Cfg| {
                    ui.display_and_log_err(err);
                    reset_app_state(&mut ini, &game_dir, Some(loader_dir), Some(&unknown_orders), ui.as_weak());
                };
                let mut found_mod = match ini.get_mod(&key, &game_dir, Some(&order_map)) {
                    Ok(found_data) => found_data,
                    Err(err) => {
                        reset_app_state_hook(err, ini);
                        return;
                    }
                };
                if found_mod.files.dll.iter().any(FileData::is_disabled) {
                    if let Err(err) = toggle_files(&game_dir, true, &mut found_mod, None) {
                        let error = format!("Failed to set mod to enabled state on removal\naborted before removal\n\n{err}");
                        error!("{error}");
                        ui.display_msg(&error);
                        return;
                    }
                }
                match confirm_remove_mod(ui.as_weak(), &game_dir, loader.path(), &found_mod, ini_dir).await {
                    Ok(_) => {
                        let success = format!("{key} uninstalled, all associated files were removed");
                        info!("{success}");
                        messages.push(success);
                        ui.global::<MainLogic>().set_current_subpage(0);
                    },
                    Err(err) => {
                        match err.kind() {
                            ErrorKind::ConnectionAborted => info!("{err}"),
                            ErrorKind::Interrupted => {
                                info!("{err}");
                                return;
                            },
                            _ => {
                                reset_app_state_hook(err, ini);
                                return;
                            }
                        }
                        ui.global::<MainLogic>().set_current_subpage(0);
                        let deregister = format!("De-registered mod: {key}");
                        info!("{deregister}");
                        messages.push(deregister);
                        messages.push(err.to_string());
                    }
                }
                if let Err(err) = ini.update() {
                    reset_app_state_hook(err, ini);
                    return;
                };
                if found_mod.order.set {
                    if let Err(err) = loader.update() {
                        reset_app_state_hook(err, ini);
                        return;
                    }
                }
                let (dlls, order_count, _) = ini.dll_set_order_count(loader.mut_section());
                let model = ui.global::<MainLogic>().get_current_mods();
                let mut_model = model.as_any().downcast_ref::<VecModel<DisplayMod>>().expect("we set this type earlier");
                mut_model.remove(row as usize);
                if found_mod.order.set {
                    let mut ord_meta_data = None;
                    loader.verify_keys(&dlls, order_count).unwrap_or_else(|key_err| {
                        if let Some(unknown_keys) = key_err.unknown_keys {
                            *unknown_orders = unknown_keys;
                        }
                        match key_err.err.kind() {
                            ErrorKind::Other => info!("{}", key_err.err),
                            ErrorKind::Unsupported => {
                                warn!("{}", key_err.err);
                                ord_meta_data = key_err.update_ord_data;
                            },
                            _ => error!("{}", key_err.err),
                        }
                        messages.push(key_err.err.to_string());
                    });
                    if ord_meta_data.is_none() {
                        ord_meta_data = Some(loader.update_order_entries(None, &unknown_orders));
                        if let Err(err) = loader.write_to_file() {
                            error!("{err}");
                            ui.display_msg(&err.to_string());
                            let _ = receive_msg().await;
                            reset_app_state(&mut ini, &game_dir, Some(loader_dir), Some(&unknown_orders), ui.as_weak());
                            return;
                        }
                    }
                    order_map = loader.parse_into_map();
                    let ord_meta_data = ord_meta_data.expect("is_some");
                    ui.global::<MainLogic>().set_max_order(MaxOrder::from(ord_meta_data.max_order));
                    model.update_order(None, &order_map, &unknown_orders, ui.as_weak());
                    if let Some(ref vals) = ord_meta_data.missing_vals {
                        let msg = DisplayMissingOrd(vals).to_string();
                        info!("{msg}");
                        messages.push(msg);
                    }
                }
                for message in messages {
                    ui.display_msg(&message);
                    let _ = receive_msg().await;
                }
            }).unwrap();
        }
    });
    ui.global::<SettingsLogic>().on_toggle_theme({
        let ui_handle = ui.as_weak();
        move |state| {
            let span = info_span!("toggle_theme");
            let _guard = span.enter();
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
            let _guard = span.enter();

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
            let _guard = span.enter();

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
            let _guard = span.enter();

            let ui = ui_handle.unwrap();
            let value = if state { "1" } else { "0" };
            let ext_ini = get_loader_ini_dir();
            if let Err(err) = save_value_ext(ext_ini, LOADER_SECTIONS[0], LOADER_KEYS[1], value) {
                error!("{err}");
                ui.display_msg(&err.to_string());
                return !state;
            }
            info!("Show terminal set to: {}", state);
            state
        }
    });
    ui.global::<SettingsLogic>().on_set_load_delay({
        let ui_handle = ui.as_weak();
        move |time| {
            let span = info_span!("set_load_delay");
            let _guard = span.enter();

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
            info!("Load delay set to: {}", DisplayTime(&time));
            ui.global::<SettingsLogic>()
                .set_load_delay(SharedString::from(DisplayTime(time).to_string()));
            ui.global::<SettingsLogic>().set_delay_input(SharedString::new());
        }
    });
    ui.global::<SettingsLogic>().on_toggle_all({
        let ui_handle = ui.as_weak();
        move |state| -> bool {
            let span = info_span!("toggle_all");
            let _guard = span.enter();

            let ui = ui_handle.unwrap();
            let game_dir = get_or_update_game_dir(None);
            let loader = ModLoader::properties(&game_dir).unwrap_or_else(|err| {
                ui.display_msg(&err.to_string());
                error!("{err}");
                ModLoader::new(!state)
            });
            if loader.anti_cheat_enabled() {
                ui.display_msg(&DisplayAntiCheatMsg.to_string());
                ui.global::<SettingsLogic>().set_loader_disabled(true);
                return !state;
            }
            let files = if loader.disabled() {
                vec![PathBuf::from(LOADER_FILES[0])]
            } else {
                vec![PathBuf::from(LOADER_FILES[1])]
            };
            let mut main_dll = RegMod::new(LOADER_FILES[1], !loader.disabled(), files);
            toggle_files(&game_dir, !state, &mut main_dll, None)
                .map(|_| state)
                .unwrap_or_else(|err| {
                    error!("{err}");
                    ui.display_msg(&format!("{err}"));
                    !state
                })
        }
    });
    ui.global::<SettingsLogic>().on_open_game_dir({
        let ui_handle = ui.as_weak();
        move || {
            let span = info_span!("open_game_dir");
            let _guard = span.enter();

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
                .unwrap_or_else(|err| {
                    let span = info_span!("send_message");
                    let _guard = span.enter();
                    error!("Failed to send message: {:?}, over channel", err.0.message);
                });
        }
    });
    ui.global::<SettingsLogic>().on_scan_for_mods({
        let ui_handle = ui.as_weak();
        move || {
            let ui = ui_handle.unwrap();
            slint::spawn_local(async move {
                let span = info_span!("scan_for_mods");
                let _guard = span.enter();
                let game_dir = get_or_update_game_dir(None);
                if let Err(err) = confirm_scan_mods(ui.as_weak(), &game_dir, None, None).await {
                    ui.display_and_log_err(err);
                };
            })
            .unwrap();
        }
    });
    ui.global::<MainLogic>().on_add_remove_order({
        let ui_handle = ui.as_weak();
        move |state, key, value, row| -> i32 {
            let span = info_span!("add_remove_order");
            let _guard = span.enter();

            let ui = ui_handle.unwrap();
            let cfg_dir = get_loader_ini_dir();
            let mut load_order = match ModLoaderCfg::read(cfg_dir) {
                Ok(data) => data,
                Err(err) => {
                    ui.display_and_log_err(err);
                    return ERROR_VAL;
                }
            };
            let load_orders = load_order.mut_section();
            let stable_k = if state {
                load_orders.insert(&key, value.to_string());
                Some(key.as_str())
            } else {
                if !load_orders.contains_key(&key) {
                    warn!("Could not find key: {key}, in: {}", LOADER_FILES[3]);
                    return ERROR_VAL;
                }
                load_orders.remove(&key);
                None
            };
            let unknown_orders = get_unknown_orders();
            let ord_meta_data = load_order.update_order_entries(stable_k, &unknown_orders);
            if let Err(err) = load_order.write_to_file() {
                error!("{err}");
                ui.display_msg(&format!(
                    "Failed to write to \"mod_loader_config.ini\"\n{err}"
                ));
                return ERROR_VAL;
            };
            let new_orders = load_order.parse_into_map();
            ui.global::<MainLogic>()
                .set_max_order(MaxOrder::from(ord_meta_data.max_order));
            let model = ui.global::<MainLogic>().get_current_mods();
            let mut selected_mod =
                model.row_data(row as usize).expect("front end gives us valid row");
            selected_mod.order.set = state;
            if !state {
                selected_mod.order.at = 0;
                if selected_mod.dll_files.row_count() != 1 {
                    selected_mod.order.i = -1;
                }
                info!("Load order removed for {}", key);
            } else {
                let new_val = *new_orders.get(&key.to_string()).expect("key inserted") as i32;
                selected_mod.order.at = new_val;
                info!("Load order set to {}, for {}", new_val, key);
            }

            model.set_row_data(row as usize, selected_mod);
            model.update_order(Some(row), &new_orders, &unknown_orders, ui.as_weak());

            if let Some(ref vals) = ord_meta_data.missing_vals {
                let msg = DisplayMissingOrd(vals).to_string();
                ui.display_msg(&msg);
                info!("{msg}");
                // because of the unsupported two way bindings with array structures in slint `update_order(..)`
                // always re-renders the state of the UI order elements
            }
            OK_VAL
        }
    });
    ui.global::<MainLogic>().on_modify_order({
        let ui_handle = ui.as_weak();
        move |to_k, from_k, value, row, dll_i| -> i32 {
            let span = info_span!("modify_order");
            let _guard = span.enter();

            let ui = ui_handle.unwrap();
            let cfg_dir = get_loader_ini_dir();
            let mut load_order = match ModLoaderCfg::read(cfg_dir) {
                Ok(data) => data,
                Err(err) => {
                    ui.display_and_log_err(err);
                    return ERROR_VAL;
                }
            };
            let load_orders = load_order.mut_section();
            let from_k_removed = if to_k != from_k && load_orders.contains_key(&from_k) {
                load_orders.remove(&from_k);
                load_orders.append(&to_k, value.to_string());
                true
            } else if load_orders.contains_key(&to_k) {
                load_orders.insert(&to_k, value.to_string());
                false
            } else {
                load_orders.append(&to_k, value.to_string());
                false
            };

            let model = ui.global::<MainLogic>().get_current_mods();
            let mut selected_mod =
                model.row_data(row as usize).expect("front end gives us valid row");
            if to_k != from_k {
                selected_mod.order.i = dll_i;
                if !selected_mod.order.set {
                    selected_mod.order.set = true
                }
                if from_k_removed {
                    if let Err(err) = load_order.write_to_file() {
                        error!("{err}");
                        ui.display_msg(&format!(
                            "Failed to write to: '{}'\n{err}",
                            LOADER_FILES[3]
                        ));
                        return ERROR_VAL;
                    };
                    model.set_row_data(row as usize, selected_mod);
                    info!("Load order set to {}, for {}", value, to_k);
                    return OK_VAL;
                }
            }

            let unknown_orders = get_unknown_orders();
            let ord_meta_data = load_order.update_order_entries(Some(&to_k), &unknown_orders);
            if let Err(err) = load_order.write_to_file() {
                error!("{err}");
                ui.display_msg(&format!(
                    "Failed to write to \"mod_loader_config.ini\"\n{err}"
                ));
                return ERROR_VAL;
            };
            let new_orders = load_order.parse_into_map();
            let new_val = *new_orders.get(&to_k.to_string()).expect("key inserted") as i32;
            selected_mod.order.at = new_val;
            ui.global::<MainLogic>()
                .set_max_order(MaxOrder::from(ord_meta_data.max_order));
            model.set_row_data(row as usize, selected_mod);
            model.update_order(Some(row), &new_orders, &unknown_orders, ui.as_weak());

            if let Some(ref vals) = ord_meta_data.missing_vals {
                let msg = DisplayMissingOrd(vals).to_string();
                ui.display_msg(&msg);
                info!("{msg}");
                return OK_VAL;
            }
            info!("Load order set to {}, for {}", new_val, to_k);
            OK_VAL
        }
    });
    ui.global::<MainLogic>().on_force_deserialize({
        let ui_handle = ui.as_weak();
        move || {
            let span = info_span!("force_deserialize");
            let _guard = span.enter();

            reset_app_state(
                &mut Cfg::default(get_ini_dir()),
                &get_or_update_game_dir(None),
                None,
                None,
                ui_handle.clone(),
            );
            info!("Re-loaded all mods after encountered error");
        }
    });

    ui.invoke_focus_app();
    ui.run().unwrap();
}

trait Sortable {
    fn update_order(
        &self,
        selected_row: Option<i32>,
        order_map: &OrderMap,
        unknown_orders: &HashSet<String>,
        ui_handle: slint::Weak<App>,
    );
}

impl Sortable for ModelRc<DisplayMod> {
    fn update_order(
        &self,
        selected_row: Option<i32>,
        order_map: &OrderMap,
        unknown_orders: &HashSet<String>,
        ui_handle: slint::Weak<App>,
    ) {
        let order_map_len = order_map.len();
        if order_map_len == 0 {
            return;
        }
        let ui = ui_handle.unwrap();
        let selected_key = selected_row.map(|row| {
            self.row_data(row as usize)
                .expect("front end gives us a valid row")
                .name
        });
        let mut unsorted_idx = (0..self.row_count()).collect::<Vec<_>>();
        let mut possible_vals = HashSet::with_capacity(order_map_len);
        let mut order_counts = vec![0_usize; order_map_len + 1];
        let Some(low_order) = order_map
            .iter()
            .filter(|(k, _)| !unknown_orders.contains(*k))
            .map(|(_, v)| {
                order_counts[*v] += 1;
                possible_vals.insert(*v);
                v
            })
            .min()
        else {
            return;
        };
        assert!(*low_order < 2);
        let mut placement_rows = order_counts
            .iter()
            .enumerate()
            .fold(
                (vec![VecDeque::new(); possible_vals.len()], 0_usize),
                |(mut placement_rows, mut counter), (i, &e)| {
                    for _ in 0..e {
                        placement_rows[i - low_order].push_back(counter);
                        counter += 1;
                    }
                    (placement_rows, counter)
                },
            )
            .0;
        let (mut i, mut selected_i, mut no_order_count) = (0_usize, 0_usize, 0_usize);
        let mut row_swapped = false;
        let mut seen_names = HashSet::new();
        while !unsorted_idx.is_empty() && no_order_count != unsorted_idx.len() {
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
                let placement_i = *new_order.unwrap() - *low_order;
                let new_order = *new_order.unwrap() as i32;
                if let Some(index) =
                    placement_rows[placement_i].iter().position(|&x| x == unsorted_i)
                {
                    if let Some(ref key) = selected_key {
                        if curr_row.name == key {
                            selected_i = unsorted_i;
                        }
                    }
                    if curr_row.order.at != new_order {
                        curr_row.order.at = new_order;
                        self.set_row_data(unsorted_i, curr_row);
                    }
                    match index {
                        0 => placement_rows[placement_i].pop_front(),
                        i if i == placement_rows[placement_i].len() - 1 => {
                            placement_rows[placement_i].pop_back()
                        }
                        _ => placement_rows[placement_i].remove(index),
                    };
                    unsorted_idx.swap_remove(i);
                    continue;
                }
                let swap_i = placement_rows[placement_i]
                    .pop_front()
                    .expect("placement_rows can not be empty if unsorted_idx is not empty");
                let swap_row = self.row_data(swap_i).expect("placement rows contains valid rows");
                if let Some(ref key) = selected_key {
                    if swap_row.name == key {
                        selected_i = unsorted_i;
                    } else if curr_row.name == key {
                        selected_i = swap_i;
                    }
                }
                if curr_row.order.at != new_order {
                    curr_row.order.at = new_order;
                }
                self.set_row_data(swap_i, curr_row);
                self.set_row_data(unsorted_i, swap_row);
                let found_i = unsorted_idx.iter().position(|x| *x == swap_i).expect(
                    "unsorted_idx & placement_rows contain the same entries and are kept in sync",
                );
                row_swapped = true;
                unsorted_idx.swap_remove(found_i);
                continue;
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
            i += 1;
        }
        if selected_row.is_some() {
            ui.invoke_update_mod_index(selected_i as i32, 1);
        }
        if row_swapped {
            ui.invoke_redraw_checkboxes();
        }
        ui.global::<MainLogic>().invoke_redraw_order_elements();
    }
}

enum Buttons {
    YesNo,
    OkCancel,
}

impl From<Buttons> for bool {
    #[inline]
    fn from(value: Buttons) -> Self {
        match value {
            Buttons::YesNo => true,
            Buttons::OkCancel => false,
        }
    }
}

impl App {
    fn display_msg(&self, msg: &str) {
        self.set_display_message(SharedString::from(msg));
        self.invoke_show_error_popup();
    }

    fn display_confirm(&self, msg: &str, buttons: Buttons) {
        self.set_alt_std_buttons(bool::from(buttons));
        self.set_display_message(SharedString::from(msg));
        self.invoke_show_confirm_popup();
    }

    fn display_and_log_err(&self, err: std::io::Error) {
        let err_str = err.to_string();
        error!("{err_str}");
        self.display_msg(&err_str);
    }
}

impl From<(usize, bool)> for MaxOrder {
    #[inline]
    fn from(value: (usize, bool)) -> Self {
        MaxOrder {
            val: value.0 as i32,
            duplicate_high_order: value.1,
        }
    }
}

impl From<&RegMod> for LoadOrder {
    fn from(value: &RegMod) -> Self {
        LoadOrder {
            at: if !value.order.set { 0 } else { value.order.at as i32 },
            i: if !value.order.set && value.files.dll.len() != 1 {
                -1
            } else {
                value.order.i as i32
            },
            set: value.order.set,
        }
    }
}

struct MessageData {
    message: Message,
    key: u32,
}

async fn receive_msg() -> Message {
    let key = GLOBAL_NUM_KEY.fetch_add(1, Ordering::SeqCst) + 1;
    let mut receiver = RECEIVER.get().unwrap().write().await;
    while let Some(msg) = receiver.recv().await {
        if msg.key == key {
            return msg.message;
        }
    }
    Message::Esc
}

/// workaround for whatever bug in rfd that doesn't interact well with the app when a user  
/// performs a secondary action within the file dialog
fn rfd_hang_workaround(window: &slint::Window) {
    let mut size = window.size();
    size.height += 1;
    window.set_size(size);
    size.height -= 1;
    window.set_size(size);
}

fn get_user_folder(path: &Path, ui_window: &slint::Window) -> std::io::Result<PathBuf> {
    let f_result = match rfd::FileDialog::new()
        .set_directory(path)
        .set_parent(&ui_window.window_handle())
        .pick_folder()
    {
        Some(file) => {
            trace!("User Selected Path: \"{}\"", file.display());
            Ok(file)
        }
        None => new_io_error!(ErrorKind::InvalidInput, "No Path Selected"),
    };
    rfd_hang_workaround(ui_window);
    f_result
}

fn get_user_files(path: &Path, ui_window: &slint::Window) -> std::io::Result<Vec<PathBuf>> {
    let f_result = match rfd::FileDialog::new()
        .set_directory(path)
        .set_parent(&ui_window.window_handle())
        .pick_files()
    {
        Some(files) => {
            let restricted_files = RESTRICTED_FILES.get().unwrap();
            if files
                .iter()
                .any(|file| restricted_files.contains(file.file_name().expect("has valid name")))
            {
                new_io_error!(ErrorKind::InvalidData, "Tried to add a restricted file")
            } else {
                trace!("User Selected Files: {files:?}");
                Ok(files)
            }
        }
        None => new_io_error!(ErrorKind::InvalidInput, "No Files Selected"),
    };
    rfd_hang_workaround(ui_window);
    f_result
}

#[inline]
fn get_ini_dir() -> &'static PathBuf {
    static CONFIG_PATH: OnceLock<PathBuf> = OnceLock::new();
    CONFIG_PATH.get_or_init(|| {
        let exe_dir = std::env::current_dir().expect("Failed to get current dir");
        exe_dir.join(INI_NAME)
    })
}

#[inline]
fn get_loader_ini_dir() -> &'static PathBuf {
    static LOADER_CONFIG_PATH: OnceLock<PathBuf> = OnceLock::new();
    LOADER_CONFIG_PATH.get_or_init(|| get_or_update_game_dir(None).join(LOADER_FILES[3]))
}

fn get_or_update_game_dir(
    update: Option<PathBuf>,
) -> tokio::sync::RwLockReadGuard<'static, PathBuf> {
    static GAME_DIR: OnceLock<RwLock<PathBuf>> = OnceLock::new();

    if let Some(path) = update {
        let gd = GAME_DIR.get_or_init(|| RwLock::new(PathBuf::new()));
        let mut gd_lock = gd.blocking_write();
        *gd_lock = path;
    }

    GAME_DIR.get().unwrap().blocking_read()
}

#[inline]
fn get_mut_unknown_orders() -> tokio::sync::RwLockWriteGuard<'static, HashSet<String>> {
    UNKNOWN_ORDER_KEYS
        .get_or_init(|| RwLock::new(HashSet::new()))
        .blocking_write()
}

#[inline]
fn get_unknown_orders() -> tokio::sync::RwLockReadGuard<'static, HashSet<String>> {
    UNKNOWN_ORDER_KEYS
        .get_or_init(|| RwLock::new(HashSet::new()))
        .blocking_read()
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
                        "Failed to open config file: '{}'\n\nError: {err}",
                        file_clone.display()
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

/// **Note:** call to find unknown_orders is blocking, so you must give a ref to unknown_orders  
/// if you currently have access to the global set
#[instrument(level = "trace", skip_all, fields(path))]
fn order_data_or_default(
    ui_handle: slint::Weak<App>,
    from_path: Option<&Path>,
    unknown_orders: Option<&HashSet<String>>,
) -> OrderMap {
    let ui = ui_handle.unwrap();
    let path = from_path.unwrap_or_else(|| get_loader_ini_dir());

    #[cfg(debug_assertions)]
    tracing::Span::current().record("path", tracing::field::display(path.display()));

    match ModLoaderCfg::read(path) {
        Ok(mut data) => {
            let mut _guard_unknown_orders = None;
            let unknown_orders = unknown_orders.unwrap_or_else(|| {
                _guard_unknown_orders = Some(get_unknown_orders());
                _guard_unknown_orders.as_ref().unwrap()
            });
            data.parse_section(unknown_orders).unwrap_or_else(|err| {
                ui.display_and_log_err(err);
                HashMap::new()
            })
        }
        Err(err) => {
            ui.display_and_log_err(err);
            HashMap::new()
        }
    }
}

/// forces all data to be re-read from file, it is fine to pass in a `Cfg::default()` here  
/// **Note:** call to find unknown_orders is blocking, so you must give a ref to unknown_orders  
/// if you currently have access to the global set
#[instrument(level = "trace", skip_all)]
fn reset_app_state(
    cfg: &mut Cfg,
    game_dir: &Path,
    loader_dir: Option<&Path>,
    unknown_orders: Option<&HashSet<String>>,
    ui_handle: slint::Weak<App>,
) {
    let ui = ui_handle.unwrap();
    ui.global::<MainLogic>().set_current_subpage(0);
    cfg.update().unwrap_or_else(|err| {
        let dsp_err = "Failed to read config data from file";
        error!("{dsp_err} {err}");
        ui.display_msg(dsp_err);
        cfg.empty_contents();
    });
    let order_data = order_data_or_default(ui.as_weak(), loader_dir, unknown_orders);
    let collected_mods = cfg.collect_mods(game_dir, Some(&order_data), false);
    ui.global::<MainLogic>()
        .set_max_order(MaxOrder::from(collected_mods.mods.max_order()));
    deserialize_collected_mods(&collected_mods, ui.as_weak());
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
        dll_files.extend(
            split_files.dll.iter().map(|f| {
                SharedString::from(omit_off_state(file_name_from_str(&f.to_string_lossy())))
            }),
        );
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
    const ELIDE_LEN: usize = 20;

    let (files, dll_files, config_files) = deserialize_split_files(&mod_data.files);
    let name = mod_data.name.replace('_', " ");
    DisplayMod {
        displayname: SharedString::from(if mod_data.name.chars().count() > ELIDE_LEN {
            name.chars().take(ELIDE_LEN - 3).chain("...".chars()).collect()
        } else {
            name.clone()
        }),
        name: SharedString::from(name),
        enabled: mod_data.state,
        files,
        config_files,
        dll_files,
        order: LoadOrder::from(mod_data),
    }
}

#[instrument(level = "trace", skip_all)]
fn deserialize_collected_mods(data: &CollectedMods, ui_handle: slint::Weak<App>) {
    let ui = ui_handle.unwrap();
    if let Some(ref warning) = data.warnings {
        ui.display_msg(&warning.to_string());
    }

    let display_mods: Rc<VecModel<DisplayMod>> = Default::default();
    data.mods
        .iter()
        .for_each(|mod_data| display_mods.push(deserialize_mod(mod_data)));

    ui.global::<MainLogic>().set_current_mods(ModelRc::from(display_mods));
    ui.global::<MainLogic>()
        .set_max_order(MaxOrder::from(data.mods.max_order()));
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
        Buttons::YesNo,
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
        Buttons::YesNo,
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
        install_files.display_paths), Buttons::YesNo);
    let result = match receive_msg().await {
        Message::Confirm => match get_user_folder(&install_files.parent_dir, ui.window()) {
            Ok(path) => {
                install_files
                    .update_fields_with_new_dir(&path, utils::installer::DisplayItems::Limit(9))
                    .await
            }
            Err(err) => Err(err),
        },
        Message::Deny => Ok(()),
        Message::Esc => new_io_error!(ErrorKind::ConnectionAborted, "Mod install canceled"),
    };
    if let Err(err) = result {
        if err.kind() == ErrorKind::InvalidInput {
            ui.display_msg(&err.to_string());
            let _ = receive_msg().await;
            let reselect_dir =
                Box::pin(async { add_dir_to_install_data(install_files, ui_handle).await });
            return reselect_dir.await;
        }
        return Err(err);
    }
    confirm_install(install_files, ui_handle).await
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
        Buttons::OkCancel,
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
    ui.display_msg(&format!("Installed mod: {}", &install_files.name));
    Ok(zip.iter().map(|(_, to_path)| to_path.to_path_buf()).collect())
}

#[instrument(level = "trace", skip_all, fields(mod_name = reg_mod.name))]
async fn confirm_remove_mod(
    ui_handle: slint::Weak<App>,
    game_dir: &Path,
    loader_dir: &Path,
    reg_mod: &RegMod,
    ini_dir: &Path,
) -> std::io::Result<()> {
    let ui = ui_handle.unwrap();
    let Some(install_dir) = reg_mod
        .files
        .chain_all()
        .min_by_key(|file| file.ancestors().count())
        .and_then(|path| Some(game_dir.join(path.parent()?)))
    else {
        return new_io_error!(ErrorKind::InvalidData, "Failed to create an install_dir");
    };

    let match_user_msg = || async {
        let esc_result = new_io_error!(ErrorKind::Interrupted, "De-registration canceled");
        match receive_msg().await {
            Message::Confirm => Ok(()),
            Message::Deny => {
                if reg_mod.order.set {
                    ui.display_confirm(
                        &format!(
                            "Do you want to remove the set load order of: {}?\n\n\
                            Note: order entries that are set within: {}, and not registered \
                            with this app are always stored as `greatest registered order + 1`. \
                            If you want to manually set an order for a mod not registered with this app \
                            add a 'load.txt' to the mods config folder. 'load.txt' files are never modified.",
                            reg_mod.order.at,
                            LOADER_FILES[3]
                        ),
                        Buttons::YesNo,
                    );
                    match receive_msg().await {
                        Message::Confirm => remove_order_entry(reg_mod, loader_dir)?,
                        Message::Deny => (),
                        Message::Esc => return esc_result,
                    }
                }
                reg_mod.remove_from_file(ini_dir)?;
                new_io_error!(
                    ErrorKind::ConnectionAborted,
                    format!(
                        "Files registered with: {}, are still installed at: '{}'",
                        DisplayName(&reg_mod.name),
                        install_dir.display()
                    )
                )
            }
            Message::Esc => esc_result,
        }
    };

    ui.display_confirm(
        "Do you want to remove mod files from the game directory?",
        Buttons::YesNo,
    );
    match_user_msg().await?;

    ui.display_confirm(
        "This is a distructive action. Are you sure you want to continue?",
        Buttons::OkCancel,
    );
    match_user_msg().await?;

    reg_mod.remove_from_file(ini_dir)?;
    remove_mod_files(game_dir, loader_dir, reg_mod)
}

#[instrument(level = "trace", skip_all)]
/// **Note:** contains a blocking read of global UNKNOWN_ORDER_KEYS
async fn confirm_scan_mods(
    ui_handle: slint::Weak<App>,
    game_dir: &Path,
    ini: Option<&Cfg>,
    order_map: Option<&OrderMap>,
) -> std::io::Result<()> {
    let ui = ui_handle.unwrap();

    ui.display_confirm(
        "Would you like to attempt to auto-import already installed mods to Elden Mod Loader GUI?",
        Buttons::YesNo,
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
        _new_map = Some(order_data_or_default(ui.as_weak(), Some(loader_dir), None));
        _new_map.as_ref().unwrap()
    });

    let mut old_mods = if ini.mods_is_empty() {
        Vec::new()
    } else {
        ui.display_confirm("Warning: This action will reset current registered mods, are you sure you want to continue?", Buttons::YesNo);
        if receive_msg().await != Message::Confirm {
            return Ok(());
        };

        let data = ini.collect_mods(game_dir, Some(order_map), false);
        if let Some(warning) = data.warnings {
            ui.display_msg(&warning.to_string());
        }

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
        data.mods
    };

    let new_mods = match scan_for_mods(game_dir, ini.path()) {
        Ok(len) => {
            let new_ini = Cfg::read(ini.path())?;
            ui.global::<MainLogic>().set_current_subpage(0);
            let mut unknown_orders = get_mut_unknown_orders();
            let order_data =
                order_data_or_default(ui.as_weak(), Some(loader_dir), Some(&unknown_orders));
            let new_mods = new_ini.collect_mods(game_dir, Some(&order_data), false);
            new_mods.mods.iter().for_each(|m| {
                m.files
                    .dll
                    .iter()
                    .filter_map(|f| f.file_name().and_then(|o| o.to_str()).map(omit_off_state))
                    .for_each(|f| {
                        unknown_orders.remove(f);
                    })
            });
            deserialize_collected_mods(&new_mods, ui.as_weak());
            ui.display_msg(&format!("Found {len} mod(s)"));
            new_mods
        }
        Err(err) => {
            ui.display_msg(&format!("{err}"));
            CollectedMods::default()
        }
    };
    if let Some(warning) = new_mods.warnings {
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

        // unsure if we want to remove order data, currently on mod removal user decides to remove,
        // or, is deleted on mod uninstallation
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
            .try_for_each(|m| toggle_files(game_dir, true, m, None))?;
    }
    Ok(())
}

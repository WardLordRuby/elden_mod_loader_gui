pub mod ini_tools {
    pub mod parser;
    pub mod writer;
}

use ini::Ini;
use ini_tools::{
    parser::IniProperty,
    writer::{remove_array, save_bool, save_path, save_path_bufs},
};
use log::{error, info, trace, warn};

use std::{
    io,
    path::{Path, PathBuf},
};

const DEFAULT_GAME_DIR: [&str; 6] = [
    "Program Files (x86)",
    "Steam",
    "steamapps",
    "common",
    "ELDEN RING",
    "Game",
];
pub const REQUIRED_GAME_FILES: [&str; 3] = [
    "eldenring.exe",
    "oo2core_6_win64.dll",
    "eossdk-win64-shipping.dll",
];

pub fn shorten_paths(
    paths: Vec<PathBuf>,
    remove: &PathBuf,
) -> Result<Vec<PathBuf>, std::path::StripPrefixError> {
    paths
        .into_iter()
        .map(|path| path.strip_prefix(remove).map(|p| p.to_path_buf()))
        .collect()
}

pub fn toggle_files(
    key: &str,
    game_dir: &Path,
    new_state: bool,
    file_paths: Vec<PathBuf>,
    save_file: &Path,
) -> Result<(), ini::Error> {
    /// Takes in a potential pathBuf, finds file_name name and outputs the new_state version
    fn toggle_name_state(file_paths: &[PathBuf], new_state: bool) -> Vec<PathBuf> {
        file_paths
            .iter()
            .map(|path| {
                let off_state = ".disabled";
                let file_name = match path.file_name() {
                    Some(name) => name,
                    None => path.as_os_str(),
                };
                let mut new_name = file_name.to_string_lossy().to_string();
                if let Some(index) = new_name.to_lowercase().find(off_state) {
                    if new_state {
                        new_name.replace_range(index..index + off_state.len(), "");
                    }
                } else if !new_state {
                    new_name.push_str(off_state);
                }
                let mut new_path = path.clone();
                new_path.set_file_name(new_name);
                new_path
            })
            .collect()
    }
    fn join_paths(base_path: PathBuf, join_to: Vec<PathBuf>) -> Vec<PathBuf> {
        join_to.iter().map(|path| base_path.join(path)).collect()
    }
    fn rename_files(
        num_files: &usize,
        paths: Vec<PathBuf>,
        new_paths: Vec<PathBuf>,
    ) -> Result<(), io::Error> {
        if *num_files != paths.len() || *num_files != new_paths.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Number of files and new paths must match",
            ));
        }

        paths
            .iter()
            .zip(new_paths.iter())
            .try_for_each(|(path, new_path)| {
                std::fs::rename(path, new_path)?;
                Ok(())
            })
    }
    fn update_cfg(
        num_file: &usize,
        path_to_save: Vec<PathBuf>,
        state: bool,
        key: &str,
        save_file: &Path,
    ) -> Result<(), ini::Error> {
        if *num_file == 1 {
            save_path(save_file, Some("mod-files"), key, &path_to_save[0])?;
        } else {
            remove_array(save_file, key)?;
            save_path_bufs(save_file, key, &path_to_save)?;
        }
        save_bool(save_file, Some("registered-mods"), key, state)?;
        Ok(())
    }
    let num_of_files = file_paths.len();

    let file_paths_clone = std::sync::Arc::new(file_paths.clone());
    let state_clone = std::sync::Arc::new(new_state);
    let game_dir_clone = game_dir.to_path_buf();

    let new_short_paths_thread =
        std::thread::spawn(move || toggle_name_state(&file_paths_clone, *state_clone));
    let original_full_paths_thread =
        std::thread::spawn(move || join_paths(game_dir_clone, file_paths));

    let short_path_new = new_short_paths_thread.join().unwrap_or(Vec::new());
    let full_path_new = join_paths(PathBuf::from(game_dir), short_path_new.clone());
    let full_path_original = original_full_paths_thread.join().unwrap_or(Vec::new());

    rename_files(&num_of_files, full_path_original, full_path_new)?;

    update_cfg(&num_of_files, short_path_new, new_state, key, save_file)?;
    Ok(())
}

pub fn get_cfg(input_file: &Path) -> Result<Ini, ini::Error> {
    Ini::load_from_file_noescape(input_file)
}

pub fn does_dir_contain(path: &Path, list: &[&str]) -> Result<(), io::Error> {
    match std::fs::read_dir(path) {
        Ok(_) => {
            let entries = std::fs::read_dir(path)?;
            let file_names: Vec<_> = entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.file_name())
                .filter_map(|file_name| file_name.to_str().map(String::from))
                .collect();

            let all_files_exist = list
                .iter()
                .all(|check_file| file_names.iter().any(|file_name| file_name == check_file));

            if all_files_exist {
                Ok(())
            } else {
                error!(
                    "{}",
                    format!("Failure: {list:?} not found in: \"{}\"", path.display(),)
                );
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Game files not found in selected path\n{}", path.display()),
                ))
            }
        }
        Err(err) => Err(err),
    }
}

pub enum PathResult {
    Full(PathBuf),
    Partial(PathBuf),
    None(PathBuf),
}
pub fn attempt_locate_game(file_name: &Path) -> Result<PathResult, ini::Error> {
    let config: Ini = match get_cfg(file_name) {
        Ok(ini) => {
            trace!(
                "Success: (attempt_locate_game) Read ini from \"{}\"",
                file_name.display()
            );
            ini
        }
        Err(err) => {
            error!(
                "Failure: (attempt_locate_game) Could not complete. Could not read ini from \"{}\"",
                file_name.display()
            );
            error!("Error: {err}");
            return Ok(PathResult::None(PathBuf::from("")));
        }
    };
    if let Some(path) = IniProperty::<PathBuf>::read(&config, Some("paths"), "game_dir", false)
        .and_then(|ini_property| {
            match does_dir_contain(&ini_property.value, &REQUIRED_GAME_FILES) {
                Ok(_) => Some(ini_property.value),
                Err(err) => {
                    error!("Error: {err}");
                    None
                }
            }
        })
    {
        info!("Success: \"game_dir\" from ini is valid");
        return Ok(PathResult::Full(path));
    }
    let try_locate = attempt_locate_dir(&DEFAULT_GAME_DIR).unwrap_or("".into());
    if does_dir_contain(&try_locate, &REQUIRED_GAME_FILES).is_ok() {
        info!("Success: located \"game_dir\" on drive");
        save_path(file_name, Some("paths"), "game_dir", try_locate.as_path())?;
        return Ok(PathResult::Full(try_locate));
    }
    if try_locate.components().count() > 1 {
        info!("Partial \"game_dir\" found");
        return Ok(PathResult::Partial(try_locate));
    }
    warn!("Could not locate \"game_dir\"");
    Ok(PathResult::None(try_locate))
}

fn attempt_locate_dir(target_path: &[&str]) -> Option<PathBuf> {
    let drive: String = match get_current_drive() {
        Some(drive) => drive,
        None => {
            warn!("Failed to find find current Drive. Using 'C:\\'");
            "C:\\".to_string()
        }
    };
    let drive_ref: std::rc::Rc<str> = std::rc::Rc::from(drive.clone());
    info!("Drive Found: {drive_ref}");

    match test_path_buf(PathBuf::from(drive), target_path) {
        Some(path) => Some(path),
        None => {
            if &*drive_ref == "C:\\" {
                None
            } else {
                test_path_buf(PathBuf::from("C:\\"), target_path)
            }
        }
    }
}

fn test_path_buf(mut path: PathBuf, target_path: &[&str]) -> Option<PathBuf> {
    for (index, dir) in target_path.iter().enumerate() {
        path.push(dir);
        trace!("Testing Path: {}", &path.display());
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
    let current_path = match std::env::current_dir() {
        Ok(path) => Some(path),
        Err(err) => {
            error!("{:?}", err);
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

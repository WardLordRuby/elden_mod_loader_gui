pub mod ini_tools {
    pub mod parser;
    pub mod writer;
}

use ini::Ini;
use ini_tools::{
    parser::{IniProperty, RegMod},
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

pub struct PathErrors {
    pub short_paths: Vec<PathBuf>,
    pub long_paths: Vec<PathBuf>,
    pub errs: Vec<std::path::StripPrefixError>,
}

impl PathErrors {
    fn new(size: usize) -> Self {
        PathErrors {
            short_paths: Vec::with_capacity(size),
            long_paths: Vec::with_capacity(size),
            errs: Vec::with_capacity(size),
        }
    }
}

pub fn shorten_paths(paths: &[PathBuf], remove: &PathBuf) -> Result<Vec<PathBuf>, PathErrors> {
    let mut output = Vec::with_capacity(paths.len());
    let mut errors = PathErrors::new(paths.len());
    paths
        .iter()
        .for_each(|path| match path.strip_prefix(remove) {
            Ok(file) => {
                output.push(PathBuf::from(file));
                errors.short_paths.push(PathBuf::from(file));
            }
            Err(err) => {
                errors.long_paths.push(PathBuf::from(path));
                errors.errs.push(err);
            }
        });
    if errors.errs.is_empty() {
        Ok(output)
    } else {
        Err(errors)
    }
}

pub async fn display_files_in_directory(
    directory: &Path,
    strip_prefix: Option<&Path>,
    starting_string: Option<&str>,
    cutoff: Option<usize>,
) -> Result<String, std::io::Error> {
    fn format_entries(
        output: &mut Vec<String>,
        directory: &Path,
        strip_prefix: Option<&Path>,
        cutoff: &mut Option<(usize, usize, usize)>,
        cutoff_reached: &mut bool,
    ) -> Result<(), std::io::Error> {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if let Some((stop_at, count, num_files)) = cutoff {
                if *cutoff_reached {
                    return Ok(());
                } else if count >= stop_at && !*cutoff_reached {
                    *cutoff_reached = true;
                    let remainder: i64 = *num_files as i64 - *count as i64;
                    match remainder {
                        ..=-1 => output.push(String::from(
                            "Unexpected behavior, file list might be wrong",
                        )),
                        0 => (),
                        1 => output.push(String::from("Plus 1 more file")),
                        2.. => output.push(format!("Plus {} more files...", remainder)),
                    };
                    return Ok(());
                } else {
                    *count += 1;
                }
            }
            if path.is_file() {
                if let Some(partial_path) =
                    strip_prefix.and_then(|prefix| path.strip_prefix(prefix).ok())
                {
                    if let Some(partial_path_str) = partial_path.to_str() {
                        output.push(partial_path_str.to_string());
                    }
                } else if let Some(file_name) = path.file_name() {
                    if let Some(file_name_str) = file_name.to_str() {
                        output.push(file_name_str.to_string());
                    }
                }
            } else if path.is_dir() {
                format_entries(output, &path, strip_prefix, cutoff, cutoff_reached)?
            }
        }
        Ok(())
    }
    fn count_files(count: &mut usize, directory: &Path) -> Result<(), std::io::Error> {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                *count += 1;
            } else if path.is_dir() {
                count_files(count, &path)?;
            }
        }
        Ok(())
    }
    let mut file_count: usize = 0;
    count_files(&mut file_count, directory)?;
    let mut files = Vec::with_capacity(file_count + 1);
    let mut calc_cutoff = cutoff.map_or_else(|| None, |num| Some((num, 0_usize, file_count)));
    let mut cutoff_reached = false;
    if let Some(string) = starting_string {
        files.push(string.to_string());
    }
    format_entries(
        &mut files,
        directory,
        strip_prefix,
        &mut calc_cutoff,
        &mut cutoff_reached,
    )?;
    Ok(files.join("\n"))
}

pub fn toggle_files(
    game_dir: &Path,
    new_state: bool,
    reg_mod: &RegMod,
    save_file: Option<&Path>,
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
    fn join_paths(base_path: &Path, join_to: &[PathBuf]) -> Vec<PathBuf> {
        join_to.iter().map(|path| base_path.join(path)).collect()
    }
    fn rename_files(
        num_files: &usize,
        paths: &[PathBuf],
        new_paths: &[PathBuf],
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
        path_to_save: &[PathBuf],
        state: bool,
        key: &str,
        save_file: &Path,
    ) -> Result<(), ini::Error> {
        if *num_file == 1 {
            save_path(save_file, Some("mod-files"), key, &path_to_save[0])?;
        } else {
            remove_array(save_file, key)?;
            save_path_bufs(save_file, key, path_to_save)?;
        }
        save_bool(save_file, Some("registered-mods"), key, state)?;
        Ok(())
    }
    let num_rename_files = reg_mod.files.len();
    let num_total_files = num_rename_files + reg_mod.config_files.len();

    let file_paths = std::sync::Arc::new(reg_mod.files.clone());
    let file_paths_clone = file_paths.clone();
    let game_dir_clone = game_dir.to_path_buf();

    let new_short_paths_thread =
        std::thread::spawn(move || toggle_name_state(&file_paths, new_state));
    let original_full_paths_thread =
        std::thread::spawn(move || join_paths(&game_dir_clone, &file_paths_clone));

    let mut short_path_new = new_short_paths_thread.join().unwrap_or(Vec::new());
    let full_path_new = join_paths(Path::new(game_dir), &short_path_new);
    let full_path_original = original_full_paths_thread.join().unwrap_or(Vec::new());
    short_path_new.extend(reg_mod.config_files.iter().cloned());

    rename_files(&num_rename_files, &full_path_original, &full_path_new)?;

    if save_file.is_some() {
        update_cfg(
            &num_total_files,
            &short_path_new,
            new_state,
            &reg_mod.name,
            save_file.expect("is some"),
        )?;
    }
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
            return Ok(PathResult::None(PathBuf::new()));
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

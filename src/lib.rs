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
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
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

macro_rules! new_io_error {
    ($kind:expr, $msg:expr) => {
        Err(std::io::Error::new($kind, $msg))
    };
}

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

fn check_parent_dir(input: &Path) -> Result<PathBuf, std::io::Error> {
    let valid_path = match input.metadata() {
        Ok(data) => {
            if data.is_dir() {
                check_dir_contains_files(input)?
            } else if data.is_file() {
                input.to_path_buf()
            } else {
                return new_io_error!(ErrorKind::InvalidData, "Unsuported file type");
            }
        }
        Err(_) => {
            return new_io_error!(ErrorKind::InvalidInput, "Path has no metadata");
        }
    };
    if let Some(name) = valid_path.file_name() {
        if name == "mods" {
            return Ok(valid_path);
        }
    }
    match valid_path.parent() {
        Some(parent) => Ok(PathBuf::from(parent)),
        None => new_io_error!(ErrorKind::InvalidInput, "Failed to create a parent_dir"),
    }
}

fn check_dir_contains_files(path: &Path) -> Result<PathBuf, std::io::Error> {
    if items_in_directory(path, FileType::File)? > 0 {
        return Ok(PathBuf::from(path));
    } else if items_in_directory(path, FileType::Any)? == 0 {
        return new_io_error!(
            ErrorKind::InvalidInput,
            "No files in the selected directory"
        );
    } else if items_in_directory(path, FileType::Dir)? == 1 {
        return check_dir_contains_files(&next_dir(path)?);
    } else if items_in_directory(path, FileType::Dir)? > 1 {
        return Ok(PathBuf::from(path));
    }
    new_io_error!(ErrorKind::InvalidData, "Unsuported file type")
}

enum FileType {
    File,
    Dir,
    Any,
}

macro_rules! count_f_type {
    ($metadata:ident, $count:ident, $f_type:ident) => {
        match $f_type {
            FileType::File => {
                if $metadata.is_file() {
                    $count += 1;
                }
            }
            FileType::Dir => {
                if $metadata.is_dir() {
                    $count += 1;
                }
            }
            FileType::Any => {
                if $metadata.is_file() || $metadata.is_dir() {
                    $count += 1;
                }
            }
        }
    };
}

fn items_in_directory(path: &Path, f_type: FileType) -> Result<usize, std::io::Error> {
    let mut count = 0;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        count_f_type!(metadata, count, f_type)
    }
    Ok(count)
}

fn next_dir(path: &Path) -> Result<PathBuf, std::io::Error> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            return Ok(entry.path());
        }
    }
    new_io_error!(ErrorKind::InvalidData, "No files in the selected directory")
}

#[derive(Debug, Default)]
pub struct InstallData {
    from_paths: Vec<PathBuf>,
    to_paths: Vec<PathBuf>,
    pub display_paths: String,
    pub parent_dir: PathBuf,
    pub install_dir: PathBuf,
}

impl InstallData {
    pub fn new(file_paths: Vec<PathBuf>, game_dir: &Path) -> Result<Self, std::io::Error> {
        let parent_dir = match file_paths
            .iter()
            .min_by_key(|path| path.ancestors().count())
        {
            Some(path) => check_parent_dir(path)?,
            None => return new_io_error!(ErrorKind::Other, "Failed to create a parent_dir"),
        };
        let display_paths = file_paths
            .iter()
            .map(|path| match path.strip_prefix(&parent_dir) {
                Ok(short_path) => short_path.to_string_lossy(),
                Err(_) => path.to_string_lossy(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(InstallData {
            from_paths: file_paths,
            to_paths: Vec::new(),
            display_paths,
            parent_dir,
            install_dir: game_dir.join("mods"),
        })
    }

    fn reconstruct(install_dir: PathBuf, new_directory: &Path) -> Result<Self, std::io::Error> {
        Ok(InstallData {
            from_paths: Vec::new(),
            to_paths: Vec::new(),
            display_paths: String::new(),
            parent_dir: check_parent_dir(new_directory)?,
            install_dir,
        })
    }

    pub fn zip_from_to_paths(&mut self) -> Result<Vec<(&Path, &Path)>, std::io::Error> {
        let mut err_indexes = Vec::new();
        let mut to_paths = self
            .from_paths
            .iter()
            .enumerate()
            .map(|(i, path)| match path.strip_prefix(&self.parent_dir) {
                Ok(path) => self.install_dir.join(path),
                Err(_) => {
                    err_indexes.push(i);
                    PathBuf::from("Encountered StripPrefixValue")
                }
            })
            .collect::<Vec<_>>();
        if !err_indexes.is_empty() {
            error!("Encountered StripPrefixError on var \"to_paths\" at index(s) {err_indexes:?}");
            let err_parent_path = check_parent_dir(
                err_indexes
                    .iter()
                    .map(|&i| self.from_paths.get(i).expect("index lookup to be correct"))
                    .min_by_key(|path| path.ancestors().count())
                    .expect("at least one path with an error exists"),
            )?;
            info!(
                "Attempting to fix errors with parent_dir: \"{}\"",
                err_parent_path.display()
            );
            err_indexes.iter().for_each(|&i| {
                let err_path = self.from_paths.get(i).expect("index lookup to be correct");
                to_paths[i] = self.install_dir.join(
                    err_path
                        .strip_prefix(&err_parent_path)
                        .expect("check_parent_dir works correctly"),
                );
            });
        }
        self.to_paths = to_paths;
        Ok(self
            .from_paths
            .iter()
            .map(|p| p.as_path())
            .zip(self.to_paths.iter().map(|p| p.as_path()))
            .collect::<Vec<_>>())
    }

    pub async fn update_from_path_and_display_data(
        &mut self,
        new_directory: &Path,
        cutoff: Option<usize>,
    ) -> Result<(), std::io::Error> {
        fn format_entries(
            outer_self: &mut InstallData,
            output: &mut Vec<String>,
            directory: &Path,
            cutoff: &mut Option<(usize, usize, usize)>,
            cutoff_reached: &mut bool,
        ) -> Result<(), std::io::Error> {
            for entry in std::fs::read_dir(directory)? {
                let entry = entry?;
                let path = entry.path();
                if !*cutoff_reached {
                    if let Some((stop_at, count, num_files)) = cutoff {
                        if count >= stop_at {
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
                        } else if path.is_file() {
                            *count += 1;
                            if let Ok(partial_path) = path.strip_prefix(&outer_self.parent_dir) {
                                if let Some(partial_path_str) = partial_path.to_str() {
                                    output.push(partial_path_str.to_string());
                                }
                            } else if let Some(path_str) =
                                path.file_name().expect("is_file").to_str()
                            {
                                output.push(path_str.to_string());
                            }
                        }
                    }
                }
                if path.is_file() {
                    outer_self.from_paths.push(path.to_path_buf());
                } else if path.is_dir() {
                    format_entries(outer_self, output, &path, cutoff, cutoff_reached)?
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
        let self_mutex = Arc::new(Mutex::new(std::mem::take(self)));
        let self_mutex_clone = Arc::clone(&self_mutex);
        let new_directory_arc = Arc::new(PathBuf::from(new_directory));
        let cutoff_arc = Arc::new(cutoff);
        let jh = std::thread::spawn(move || -> Result<(), std::io::Error> {
            let mut self_mutex = self_mutex_clone.lock().unwrap();

            if self_mutex
                .parent_dir
                .strip_prefix(new_directory_arc.as_ref())
                .is_ok()
            {
                if new_directory_arc.ancestors().count()
                    <= self_mutex.parent_dir.ancestors().count()
                {
                    info!("Selected directory contains the original files, reconstructing data");
                    *self_mutex = InstallData::reconstruct(
                        self_mutex.install_dir.clone(),
                        new_directory_arc.as_ref(),
                    )?;
                }
            } else if new_directory_arc
                .strip_prefix(&self_mutex.parent_dir)
                .is_ok()
            {
                info!("New directory selected contains unique files, and is inside the original_parent, entire folder will be moved");
            } else {
                info!("New directory selected contains unique files, entire folder will be moved");
                self_mutex.parent_dir = check_parent_dir(&new_directory_arc)?
            }

            let mut file_count: usize = 0;
            count_files(&mut file_count, &new_directory_arc)?;
            let num_files_to_display: usize;
            let mut calc_cutoff = match *cutoff_arc {
                Some(num) => {
                    num_files_to_display = num + 1;
                    Some((num, 0_usize, file_count))
                }
                None => {
                    num_files_to_display = file_count + 1;
                    None
                }
            };
            let mut cutoff_reached = false;
            let mut files_to_display = Vec::with_capacity(num_files_to_display);
            let from_path_clone = self_mutex.from_paths.clone();
            self_mutex.from_paths = Vec::with_capacity(file_count + from_path_clone.len());
            self_mutex.from_paths.extend(from_path_clone);
            if !self_mutex.display_paths.is_empty() {
                files_to_display.push(self_mutex.display_paths.clone());
            }

            format_entries(
                &mut self_mutex,
                &mut files_to_display,
                &new_directory_arc,
                &mut calc_cutoff,
                &mut cutoff_reached,
            )?;
            self_mutex.display_paths = files_to_display.join("\n");
            Ok(())
        });
        match jh.join() {
            Ok(result) => match result {
                Ok(_) => {
                    let mut new_self = self_mutex.lock().unwrap();
                    std::mem::swap(&mut *new_self, self);
                    Ok(())
                }
                Err(err) => Err(err),
            },
            Err(err) => new_io_error!(
                ErrorKind::BrokenPipe,
                format!("Thread failed to join\n\n{err:?}")
            ),
        }
    }
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
    ) -> Result<(), std::io::Error> {
        if *num_files != paths.len() || *num_files != new_paths.len() {
            return new_io_error!(
                ErrorKind::InvalidInput,
                "Number of files and new paths must match"
            );
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

pub fn does_dir_contain(path: &Path, list: &[&str]) -> Result<(), std::io::Error> {
    match std::fs::read_dir(path) {
        Ok(_) => {
            let entries = std::fs::read_dir(path)?;
            let file_names: Vec<_> = entries
                .filter_map(|entry| entry.ok())
                .map(|entry| String::from(entry.file_name().to_string_lossy()))
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
                new_io_error!(
                    ErrorKind::NotFound,
                    format!("Game files not found in selected path\n{}", path.display())
                )
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

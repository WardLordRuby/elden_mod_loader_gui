use log::{error, info};
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::new_io_error;

fn get_parent_dir(input: &Path) -> Result<PathBuf, std::io::Error> {
    match input.metadata() {
        Ok(data) => {
            if data.is_dir() {
                Ok(check_dir_contains_files(input)?)
            } else if data.is_file() {
                match input.parent() {
                    Some(parent) => Ok(check_dir_contains_files(parent)?),
                    None => {
                        new_io_error!(ErrorKind::InvalidData, "Failed to create a parent_dir")
                    }
                }
            } else {
                new_io_error!(ErrorKind::InvalidData, "Unsuported file type")
            }
        }
        Err(_) => {
            new_io_error!(ErrorKind::InvalidData, "Unable to retrieve metadata")
        }
    }
}

fn check_dir_contains_files(path: &Path) -> Result<PathBuf, std::io::Error> {
    let num_of_dirs = items_in_directory(path, FileType::Dir)?;
    if files_in_directory_tree(path)? == 0 {
        return new_io_error!(
            ErrorKind::InvalidInput,
            "No files in the selected directory"
        );
    } else if items_in_directory(path, FileType::File)? > 0 {
        return Ok(PathBuf::from(path));
    } else if num_of_dirs == 1 {
        return check_dir_contains_files(&next_dir(path)?);
    } else if num_of_dirs > 1 {
        let mut non_empty_branches: usize = 0;
        let mut non_empty_dirs = Vec::with_capacity(num_of_dirs);
        for entry in std::fs::read_dir(path)? {
            let dir = entry?.path();
            if files_in_directory_tree(&dir)? != 0 {
                non_empty_branches += 1;
                non_empty_dirs.push(dir);
            }
        }
        if non_empty_branches == 1 {
            return check_dir_contains_files(&non_empty_dirs[0]);
        }
        return Ok(PathBuf::from(path));
    }
    new_io_error!(ErrorKind::InvalidData, "Unsuported file type")
}

#[allow(dead_code)]
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

fn files_in_directory_tree(directory: &Path) -> Result<usize, std::io::Error> {
    fn count_loop(count: &mut usize, path: &Path) -> Result<(), std::io::Error> {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if metadata.is_symlink() {
                return new_io_error!(ErrorKind::InvalidData, "Unsuported file type");
            } else if metadata.is_file() {
                *count += 1;
            } else if metadata.is_dir() {
                count_loop(count, &entry.path())?;
            }
        }
        Ok(())
    }

    let mut count: usize = 0;
    count_loop(&mut count, directory)?;
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

#[derive(Debug, Clone)]
pub struct InstallData {
    pub name: String,
    from_paths: Vec<PathBuf>,
    to_paths: Vec<PathBuf>,
    pub display_paths: String,
    pub parent_dir: PathBuf,
    pub install_dir: PathBuf,
}

impl InstallData {
    pub fn new(
        name: &str,
        file_paths: Vec<PathBuf>,
        game_dir: &Path,
    ) -> Result<Self, std::io::Error> {
        let parent_dir = match file_paths
            .iter()
            .min_by_key(|path| path.ancestors().count())
        {
            Some(path) => get_parent_dir(path)?,
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
            name: String::from(name),
            from_paths: file_paths,
            to_paths: Vec::new(),
            display_paths,
            parent_dir,
            install_dir: game_dir.join("mods"),
        })
    }

    fn reconstruct(
        name: &str,
        install_dir: PathBuf,
        new_directory: &Path,
    ) -> Result<Self, std::io::Error> {
        Ok(InstallData {
            name: String::from(name),
            from_paths: Vec::new(),
            to_paths: Vec::new(),
            display_paths: String::new(),
            parent_dir: get_parent_dir(new_directory)?,
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
            let err_parent_path = get_parent_dir(
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
                        .expect("get_parent_dir works correctly"),
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

        let self_mutex = Arc::new(Mutex::new(self.clone()));
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
                        &self_mutex.name,
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
                self_mutex.parent_dir = get_parent_dir(&new_directory_arc)?
            }

            let file_count = files_in_directory_tree(&new_directory_arc)?;
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

use log::trace;
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::{does_dir_contain, new_io_error, parent_or_err};

/// Returns the deepest occurance of a directory that contains at least 1 file
/// use parent_or_err for a direct binding to what is one level up
fn get_parent_dir(input: &Path) -> std::io::Result<PathBuf> {
    match input.metadata() {
        Ok(data) => {
            if data.is_dir() {
                Ok(check_dir_contains_files(input)?)
            } else if data.is_file() {
                Ok(check_dir_contains_files(parent_or_err(input)?)?)
            } else {
                new_io_error!(ErrorKind::InvalidData, "Unsuported file type")
            }
        }
        Err(_) => {
            new_io_error!(ErrorKind::InvalidData, "Unable to retrieve metadata")
        }
    }
}

fn check_dir_contains_files(path: &Path) -> std::io::Result<PathBuf> {
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

fn items_in_directory(path: &Path, f_type: FileType) -> std::io::Result<usize> {
    let mut count = 0;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        match f_type {
            FileType::File => {
                if metadata.is_file() {
                    count += 1;
                }
            }
            FileType::Dir => {
                if metadata.is_dir() {
                    count += 1;
                }
            }
            FileType::Any => {
                if metadata.is_file() || metadata.is_dir() {
                    count += 1;
                }
            }
        }
    }
    Ok(count)
}

fn files_in_directory_tree(directory: &Path) -> std::io::Result<usize> {
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

fn next_dir(path: &Path) -> std::io::Result<PathBuf> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            return Ok(entry.path());
        }
    }
    new_io_error!(ErrorKind::InvalidData, "No dir in the selected directory")
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
    pub fn new(name: &str, file_paths: Vec<PathBuf>, game_dir: &Path) -> std::io::Result<Self> {
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
        let mut data = InstallData {
            name: String::from(name),
            from_paths: file_paths,
            to_paths: Vec::new(),
            display_paths,
            parent_dir,
            install_dir: game_dir.join("mods"),
        };
        data.collect_to_paths();
        Ok(data)
    }

    fn reconstruct(
        name: &str,
        install_dir: PathBuf,
        new_directory: &Path,
    ) -> std::io::Result<Self> {
        Ok(InstallData {
            name: String::from(name),
            from_paths: Vec::new(),
            to_paths: Vec::new(),
            display_paths: String::new(),
            parent_dir: get_parent_dir(new_directory)?,
            install_dir,
        })
    }
    pub fn collect_to_paths(&mut self) {
        self.to_paths.extend(
            self.from_paths
                .iter()
                .skip(self.to_paths.len())
                .filter_map(|path| path.strip_prefix(&self.parent_dir).ok())
                .map(|path| self.install_dir.join(path)),
        )
    }

    pub fn zip_from_to_paths(&self) -> std::io::Result<Vec<(&Path, &Path)>> {
        if self.from_paths.len() != self.to_paths.len() {
            return new_io_error!(
                ErrorKind::BrokenPipe,
                "collect_to_paths either failed or was not ran"
            );
        }
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
    ) -> std::io::Result<()> {
        fn format_entries(
            outer_self: &mut InstallData,
            output: &mut Vec<String>,
            directory: &Path,
            cutoff: &mut Option<(usize, usize, usize)>,
            cutoff_reached: &mut bool,
        ) -> std::io::Result<()> {
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
            let valid_dir = check_dir_contains_files(&new_directory_arc)?;
            if does_dir_contain(&valid_dir, crate::Operation::All, &["mods"])? {
                return new_io_error!(ErrorKind::InvalidData, "Invalid file structure");
            }
            let mut self_mutex = self_mutex_clone.lock().unwrap();

            if self_mutex.parent_dir.strip_prefix(&valid_dir).is_ok() {
                if valid_dir.ancestors().count() <= self_mutex.parent_dir.ancestors().count() {
                    trace!("Selected directory contains the original files, reconstructing data");
                    *self_mutex = InstallData::reconstruct(
                        &self_mutex.name,
                        self_mutex.install_dir.clone(),
                        &valid_dir,
                    )?;
                }
            } else if valid_dir.strip_prefix(&self_mutex.parent_dir).is_ok() {
                trace!("New directory selected contains unique files, and is inside the original_parent, entire folder will be moved");
                if valid_dir.ends_with("mods")
                    && items_in_directory(parent_or_err(&valid_dir)?, FileType::File)? > 0
                {
                    return new_io_error!(ErrorKind::InvalidData, "Invalid file structure");
                }
                self_mutex.parent_dir = parent_or_err(&valid_dir)?.to_path_buf()
            } else {
                trace!("New directory selected contains unique files, entire folder will be moved");
                match items_in_directory(&valid_dir, FileType::Dir)? == 0
                    && items_in_directory(&valid_dir, FileType::File)? > 1
                {
                    true => self_mutex.parent_dir = parent_or_err(&valid_dir)?.to_path_buf(),
                    false => self_mutex.parent_dir = valid_dir.clone(),
                }
            }

            let file_count = files_in_directory_tree(&valid_dir)?;
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
                &valid_dir,
                &mut calc_cutoff,
                &mut cutoff_reached,
            )?;
            self_mutex.display_paths = files_to_display.join("\n");
            if self_mutex.to_paths.is_empty() {
                self_mutex.collect_to_paths();
            }
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

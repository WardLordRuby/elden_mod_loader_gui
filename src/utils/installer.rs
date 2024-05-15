use log::trace;
use std::{
    collections::HashSet,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use crate::{
    does_dir_contain, file_name_or_err, new_io_error, parent_or_err,
    utils::ini::{
        parser::RegMod,
        writer::{remove_order_entry, save_bool, save_path, save_paths},
    },
    FileData, INI_SECTIONS,
};

/// Returns the deepest occurance of a directory that contains at least 1 file  
/// Use parent_or_err for a direct binding to what is one level up
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
    if directory_tree_is_empty(path)? {
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
            if !directory_tree_is_empty(&dir)? {
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
    fn count_loop(count: &mut usize, path: &Path) -> std::io::Result<()> {
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

fn directory_tree_is_empty(directory: &Path) -> std::io::Result<bool> {
    fn lookup_loop(path: &Path) -> std::io::Result<bool> {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if metadata.is_symlink() {
                return new_io_error!(ErrorKind::InvalidData, "Unsuported file type");
            } else if metadata.is_file() || (metadata.is_dir() && !lookup_loop(&entry.path())?) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    lookup_loop(directory)
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

fn parent_dir_from_vec(in_files: &[PathBuf]) -> std::io::Result<PathBuf> {
    match in_files.iter().min_by_key(|path| path.ancestors().count()) {
        Some(path) => get_parent_dir(path),
        None => new_io_error!(ErrorKind::Other, "Failed to create a parent_dir"),
    }
}

pub enum DisplayItems {
    Limit(usize),
    All,
    None,
}

struct Cutoff {
    reached: bool,
    has_limit: bool,
    display_count: usize,
    data: CutoffData,
}

impl Cutoff {
    fn new(input: &DisplayItems, file_count: usize) -> Self {
        match input {
            DisplayItems::All => Cutoff {
                reached: false,
                has_limit: false,
                display_count: file_count + 1,
                data: CutoffData {
                    limit: 1,
                    file_count,
                    counter: 0,
                },
            },
            DisplayItems::Limit(num) => Cutoff {
                reached: false,
                has_limit: true,
                display_count: num + 2,
                data: CutoffData {
                    limit: *num,
                    file_count,
                    counter: 0,
                },
            },
            DisplayItems::None => Cutoff {
                reached: true,
                has_limit: false,
                display_count: 1,
                data: CutoffData::default(),
            },
        }
    }
}

#[derive(Default)]
struct CutoffData {
    limit: usize,
    file_count: usize,
    counter: usize,
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
        let parent_dir = parent_dir_from_vec(&file_paths)?;
        let mut data = InstallData {
            name: String::from(name),
            from_paths: file_paths,
            to_paths: Vec::new(),
            display_paths: String::new(),
            parent_dir,
            install_dir: game_dir.join("mods"),
        };
        data.init_display_paths();
        data.collect_to_paths();
        Ok(data)
    }

    pub fn amend(
        amend_to: &RegMod,
        file_paths: Vec<PathBuf>,
        game_dir: &Path,
    ) -> std::io::Result<Self> {
        let amend_mod_split_file_names = amend_to.files.dll.iter().try_fold(
            Vec::with_capacity(amend_to.files.len()),
            |mut acc, file| {
                let file_name = file_name_or_err(file)?.to_string_lossy();
                let file_data = FileData::from(&file_name);
                acc.push((
                    String::from(file_data.name),
                    String::from(file_data.extension),
                ));
                Ok::<Vec<(String, String)>, std::io::Error>(acc)
            },
        )?;
        let mut install_dir = game_dir.join("mods");
        let dll_files = amend_mod_split_file_names
            .iter()
            .filter(|(_, ext)| ext == ".dll")
            .map(|(file_name, _)| file_name)
            .collect::<Vec<_>>();
        if dll_files.len() == 1 {
            install_dir = install_dir.join(dll_files[0]);
        } else {
            return new_io_error!(
                ErrorKind::InvalidInput,
                "Error:\nCould not determine the proper file structure for installing files"
            );
        }
        let parent_dir = parent_dir_from_vec(&file_paths)?;
        let mut data = InstallData {
            name: String::from(&amend_to.name),
            from_paths: file_paths,
            to_paths: Vec::new(),
            display_paths: String::new(),
            parent_dir,
            install_dir,
        };
        data.init_display_paths();
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

    fn init_display_paths(&mut self) {
        self.display_paths = self
            .from_paths
            .iter()
            .map(|path| match path.strip_prefix(&self.parent_dir) {
                Ok(short_path) => short_path.to_string_lossy(),
                Err(_) => path.to_string_lossy(),
            })
            .collect::<Vec<_>>()
            .join("\n");
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

    /// Use update_fields_with_new_dir when installing a mod from outside the game_dir  
    /// This function is for internal use only and contians no saftey checks
    fn import_files_from_dir(
        &mut self,
        directory: &Path,
        cutoff: &DisplayItems,
    ) -> std::io::Result<()> {
        let file_count = files_in_directory_tree(directory)?;

        let mut cut_off_data = Cutoff::new(cutoff, file_count);
        let mut files_to_display = Vec::with_capacity(cut_off_data.display_count);
        if !self.display_paths.is_empty() {
            files_to_display.push(self.display_paths.clone());
        }
        self.from_paths.reserve(file_count);

        fn format_loop(
            outer_self: &mut InstallData,
            display_data: &mut Vec<String>,
            directory: &Path,
            cutoff: &mut Cutoff,
        ) -> std::io::Result<()> {
            for entry in std::fs::read_dir(directory)? {
                let entry = entry?;
                let path = entry.path();
                let is_valid_file = match path.is_file() {
                    true => path.extension().is_some(),
                    false => false,
                };
                if !cutoff.reached && is_valid_file {
                    if cutoff.data.counter < cutoff.data.limit {
                        if cutoff.has_limit {
                            cutoff.data.counter += 1;
                        }
                        if let Ok(partial_path) = path.strip_prefix(&outer_self.parent_dir) {
                            if let Some(partial_path_str) = partial_path.to_str() {
                                display_data.push(partial_path_str.to_string());
                            }
                        } else if let Some(path_str) = path.file_name().expect("is_file").to_str() {
                            display_data.push(path_str.to_string());
                        }
                    } else {
                        cutoff.reached = true;
                        let remainder: i64 =
                            cutoff.data.file_count as i64 - cutoff.data.counter as i64;
                        match remainder {
                            ..=-1 => {
                                return new_io_error!(
                                    ErrorKind::BrokenPipe,
                                    "Unexpected behavior, remainder < 0"
                                )
                            }
                            0 => (),
                            1 => display_data.push(String::from("Plus 1 more file")),
                            2.. => display_data.push(format!("Plus {} more files...", remainder)),
                        };
                    }
                }
                if is_valid_file {
                    outer_self.from_paths.push(path.to_path_buf());
                } else if path.is_dir() {
                    format_loop(outer_self, display_data, &path, cutoff)?
                }
            }
            Ok(())
        }

        format_loop(self, &mut files_to_display, directory, &mut cut_off_data)?;

        if let DisplayItems::All | DisplayItems::Limit(_) = *cutoff {
            self.display_paths = files_to_display.join("\n");
        }

        Ok(())
    }

    /// This function is intended to add a directory to a InstallData::new()  
    /// Subsequent runs of this funciton is not tested and not expected to work
    pub async fn update_fields_with_new_dir(
        &mut self,
        new_directory: &Path,
        cutoff: DisplayItems,
    ) -> std::io::Result<()> {
        let mut self_clone = self.clone();
        let new_directory_owned = PathBuf::from(new_directory);
        let jh = std::thread::spawn(move || -> std::io::Result<InstallData> {
            let valid_dir = check_dir_contains_files(&new_directory_owned)?;
            let game_dir = self_clone.install_dir.parent().expect("has parent");
            if valid_dir.strip_prefix(game_dir).is_ok() {
                return new_io_error!(ErrorKind::InvalidInput, "Files are already installed");
            } else if matches!(
                does_dir_contain(&valid_dir, crate::Operation::All, &["mods"])?,
                crate::OperationResult::Bool(true)
            ) {
                return new_io_error!(ErrorKind::InvalidData, "Invalid file structure");
            }

            if self_clone.parent_dir.strip_prefix(&valid_dir).is_ok() {
                if valid_dir.ancestors().count() <= self_clone.parent_dir.ancestors().count() {
                    trace!("Selected directory contains the original files, reconstructing data");
                    self_clone = InstallData::reconstruct(
                        &self_clone.name,
                        self_clone.install_dir.clone(),
                        &valid_dir,
                    )?;
                }
            } else if valid_dir.strip_prefix(&self_clone.parent_dir).is_ok() {
                trace!("New directory selected contains unique files, and is inside the original_parent, entire folder will be moved");
                if valid_dir.ends_with("mods")
                    && items_in_directory(parent_or_err(&valid_dir)?, FileType::File)? > 0
                {
                    return new_io_error!(ErrorKind::InvalidData, "Invalid file structure");
                }
                self_clone.parent_dir = parent_or_err(&valid_dir)?.to_path_buf()
            } else {
                // MARK: TODO
                // This branch needs further debugging | do we ever want the false path? if so document why
                trace!("New directory selected contains unique files, entire folder will be moved");
                match items_in_directory(&valid_dir, FileType::Dir)? == 0 {
                    true => self_clone.parent_dir = parent_or_err(&valid_dir)?.to_path_buf(),
                    false => self_clone.parent_dir.clone_from(&valid_dir),
                }
            }

            self_clone.import_files_from_dir(&valid_dir, &cutoff)?;

            if self_clone.to_paths.len() != self_clone.from_paths.len() {
                self_clone.collect_to_paths();
            }
            Ok(self_clone)
        });
        match jh.join() {
            Ok(result) => match result {
                Ok(mut data) => {
                    std::mem::swap(&mut data, self);
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

pub fn remove_mod_files(
    game_dir: &Path,
    loader_dir: &Path,
    reg_mod: &RegMod,
) -> std::io::Result<()> {
    let remove_files = reg_mod
        .files
        .file_refs()
        .iter()
        .map(|f| game_dir.join(f))
        .collect::<Vec<_>>();

    if remove_files.iter().any(|file| !matches!(file.try_exists(), Ok(true))) {
        return new_io_error!(
            ErrorKind::InvalidInput,
            "Could not confirm existance of all files to remove"
        );
    };

    let mut parent_dirs = remove_files
        .iter()
        .map(|p| p.parent().expect("has parent and verified to exist"))
        .filter(|&parent| !parent.ends_with("mods") && parent != game_dir)
        .collect::<HashSet<_>>();

    for directory in parent_dirs.clone() {
        for partical_path in directory.ancestors().skip(1) {
            if partical_path == game_dir {
                break;
            }
            if partical_path.ends_with("mods") {
                continue;
            }
            if !parent_dirs.contains(partical_path) {
                parent_dirs.insert(partical_path);
            }
        }
    }

    let mut parent_dirs = parent_dirs.into_iter().collect::<Vec<_>>();
    parent_dirs.sort_by_key(|path| path.components().count());

    remove_files.iter().try_for_each(std::fs::remove_file)?;

    parent_dirs.iter().rev().try_for_each(|dir| {
        if items_in_directory(dir, FileType::Any)? == 0 {
            std::fs::remove_dir(dir)
        } else {
            Ok(())
        }
    })?;

    if reg_mod.order.set {
        remove_order_entry(reg_mod, loader_dir)?;
    }
    Ok(())
}

pub fn scan_for_mods(game_dir: &Path, ini_file: &Path) -> std::io::Result<usize> {
    let scan_dir = game_dir.join("mods");
    if !matches!(scan_dir.try_exists(), Ok(true)) {
        return new_io_error!(
            ErrorKind::BrokenPipe,
            format!(
                "\"mods\" folder does not exist in \"{}\"",
                game_dir.display()
            )
        );
    };
    let num_files = items_in_directory(&scan_dir, FileType::File)?;
    let mut file_sets = Vec::with_capacity(num_files);
    let mut files = Vec::with_capacity(num_files);
    let mut dirs = Vec::with_capacity(items_in_directory(&scan_dir, FileType::Dir)?);
    for entry in std::fs::read_dir(scan_dir)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_file() {
            files.push(entry.path())
        } else if metadata.is_dir() {
            dirs.push(entry.path())
        }
    }
    for file in files.iter() {
        let name = file_name_or_err(file)?.to_string_lossy();
        let file_data = FileData::from(&name);
        if file_data.extension != ".dll" {
            continue;
        };
        if let Some(dir) = dirs.iter().find(|d| d.file_name().expect("is dir") == file_data.name) {
            let mut data = InstallData::new(file_data.name, vec![file.clone()], game_dir)?;
            data.import_files_from_dir(dir, &DisplayItems::None)?;
            file_sets.push(RegMod::new(
                &data.name,
                file_data.enabled,
                data.from_paths
                    .into_iter()
                    .map(|p| p.strip_prefix(game_dir).expect("file found here").to_path_buf())
                    .collect::<Vec<_>>(),
            ));
        } else {
            file_sets.push(RegMod::new(
                file_data.name,
                file_data.enabled,
                vec![file.strip_prefix(game_dir).expect("file found here").to_path_buf()],
            ));
        }
    }
    for mod_data in file_sets.iter_mut() {
        save_bool(ini_file, INI_SECTIONS[2], &mod_data.name, mod_data.state)?;
        let file_refs = mod_data.files.file_refs();
        if file_refs.len() == 1 {
            save_path(ini_file, INI_SECTIONS[3], &mod_data.name, file_refs[0])?;
        } else {
            save_paths(ini_file, INI_SECTIONS[3], &mod_data.name, &file_refs)?;
        }
        mod_data.verify_state(game_dir, ini_file)?;
    }
    Ok(file_sets.len())
}

#[cfg(test)]
mod tests {
    use elden_mod_loader_gui::{ini_tools::writer::new_cfg, *};
    use std::{
        fs::{metadata, remove_file, File},
        path::{Path, PathBuf},
    };

    #[test]
    fn do_files_toggle() {
        fn file_exists(file_path: &Path) -> bool {
            if let Ok(metadata) = metadata(file_path) {
                metadata.is_file()
            } else {
                false
            }
        }

        let key = "test_files";
        let dir_to_test_files =
            Path::new("C:\\Users\\cal_b\\Documents\\School\\code\\elden_mod_loader_gui");
        let save_file = "test_files\\file_toggle_test.ini";
        new_cfg(save_file).unwrap();

        let test_files = vec![
            PathBuf::from("test_files\\test1.txt"),
            PathBuf::from("test_files\\test2.ini"),
            PathBuf::from("test_files\\test3.dll"),
            PathBuf::from("test_files\\test4.exe"),
            PathBuf::from("test_files\\test5.bin"),
        ];

        let test_files_disabled = vec![
            PathBuf::from("test_files\\test1.txt.disabled"),
            PathBuf::from("test_files\\test2.ini.disabled"),
            PathBuf::from("test_files\\test3.dll.disabled"),
            PathBuf::from("test_files\\test4.exe.disabled"),
            PathBuf::from("test_files\\test5.bin.disabled"),
        ];

        for test_file in test_files.iter() {
            File::create(test_file.to_string_lossy().to_string()).unwrap();
        }

        toggle_files(key, dir_to_test_files, false, test_files.clone(), save_file).unwrap();

        for path_to_test in test_files_disabled.iter() {
            assert!(file_exists(path_to_test.as_path()));
        }

        toggle_files(
            key,
            dir_to_test_files,
            true,
            test_files_disabled.clone(),
            save_file,
        )
        .unwrap();

        for path_to_test in test_files.iter() {
            assert!(file_exists(path_to_test.as_path()));
        }

        for test_file in test_files.iter() {
            remove_file(test_file.as_path()).unwrap();
        }
        remove_file(save_file).unwrap();
    }
}

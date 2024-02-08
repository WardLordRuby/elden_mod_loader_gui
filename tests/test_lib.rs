#[cfg(test)]
mod tests {
    use elden_mod_loader_gui::*;
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

        for path in test_files.iter() {
            let _ = File::create(path.to_string_lossy().to_string());
        }

        toggle_files(key, dir_to_test_files, false, test_files.clone(), save_file);

        assert!(file_exists(test_files_disabled[0].as_path()));
        assert!(file_exists(test_files_disabled[1].as_path()));
        assert!(file_exists(test_files_disabled[2].as_path()));
        assert!(file_exists(test_files_disabled[3].as_path()));
        assert!(file_exists(test_files_disabled[4].as_path()));

        toggle_files(
            key,
            dir_to_test_files,
            true,
            test_files_disabled.clone(),
            save_file,
        );

        assert!(file_exists(test_files[0].as_path()));
        assert!(file_exists(test_files[1].as_path()));
        assert!(file_exists(test_files[2].as_path()));
        assert!(file_exists(test_files[3].as_path()));
        assert!(file_exists(test_files[4].as_path()));

        for path in test_files.iter() {
            let _ = remove_file(path.as_path());
        }
    }
}

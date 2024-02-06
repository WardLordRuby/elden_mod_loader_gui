#[cfg(test)]
mod tests {
    use elden_mod_loader_gui::*;
    use std::{
        fs::{metadata, remove_file, File},
        path::PathBuf,
    };

    #[test]
    fn do_files_toggle() {
        fn file_exists(file_path: &str) -> bool {
            if let Ok(metadata) = metadata(file_path) {
                metadata.is_file()
            } else {
                false
            }
        }
        let test_files = vec![
            PathBuf::from("test_files\\test1.txt"),
            PathBuf::from("test_files\\test2.ini"),
            PathBuf::from("test_files\\test3.dll"),
            PathBuf::from("test_files\\test4.exe"),
            PathBuf::from("test_files\\test5.bin"),
        ];

        for path in test_files.iter() {
            let _ = File::create(path.to_string_lossy().to_string());
        }

        let _ = toggle_files(test_files.clone());

        assert!(file_exists("test_files\\test1.txt.disabled"));
        assert!(file_exists("test_files\\test2.ini.disabled"));
        assert!(file_exists("test_files\\test3.dll.disabled"));
        assert!(file_exists("test_files\\test4.exe.disabled"));
        assert!(file_exists("test_files\\test5.bin.disabled"));

        let test_files_2 = vec![
            PathBuf::from("test_files\\test1.txt.disabled"),
            PathBuf::from("test_files\\test2.ini.disabled"),
            PathBuf::from("test_files\\test3.dll.disabled"),
            PathBuf::from("test_files\\test4.exe.disabled"),
            PathBuf::from("test_files\\test5.bin.disabled"),
        ];

        let _ = toggle_files(test_files_2);

        assert!(file_exists("test_files\\test1.txt"));
        assert!(file_exists("test_files\\test2.ini"));
        assert!(file_exists("test_files\\test3.dll"));
        assert!(file_exists("test_files\\test4.exe"));
        assert!(file_exists("test_files\\test5.bin"));

        for path in test_files.iter() {
            let _ = remove_file(path.as_path());
        }
    }
}

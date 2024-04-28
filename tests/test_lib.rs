#[cfg(test)]
mod tests {
    use elden_mod_loader_gui::{
        toggle_files,
        utils::ini::{parser::RegMod, writer::new_cfg},
        OFF_STATE,
    };
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

        let dir_to_test_files =
            Path::new("C:\\Users\\cal_b\\Documents\\School\\code\\elden_mod_loader_gui");
        let save_file = Path::new("temp\\file_toggle_test.ini");
        new_cfg(save_file).unwrap();

        let test_files = vec![
            PathBuf::from("temp\\test1.txt"),
            PathBuf::from("temp\\test2.bhd"),
            PathBuf::from("temp\\test3.dll"),
            PathBuf::from("temp\\test4.exe"),
            PathBuf::from("temp\\test5.bin"),
            PathBuf::from("temp\\config.ini"),
        ];

        let test_mod = RegMod::new("Test", true, test_files.clone());
        let test_files_disabled = test_mod
            .mod_files
            .iter()
            .map(|file| PathBuf::from(format!("{}{OFF_STATE}", file.display())))
            .collect::<Vec<_>>();

        assert_eq!(test_mod.mod_files.len(), 1);
        assert_eq!(test_mod.config_files.len(), 1);
        assert_eq!(test_mod.other_files.len(), 4);

        for test_file in test_files.iter() {
            File::create(test_file.to_string_lossy().to_string()).unwrap();
        }

        toggle_files(
            dir_to_test_files,
            !test_mod.state,
            &test_mod,
            Some(save_file),
        )
        .unwrap();

        for path_to_test in test_files_disabled.iter() {
            assert!(file_exists(path_to_test.as_path()));
        }

        let test_mod = RegMod {
            name: test_mod.name,
            state: false,
            mod_files: test_files_disabled,
            config_files: test_mod.config_files,
            other_files: test_mod.other_files,
        };

        toggle_files(
            dir_to_test_files,
            !test_mod.state,
            &test_mod,
            Some(save_file),
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

#[cfg(test)]
mod tests {
    use elden_mod_loader_gui::{
        ini_tools::{
            parser::{split_out_config_files, RegMod},
            writer::new_cfg,
        },
        *,
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
        let save_file = Path::new("test_files\\file_toggle_test.ini");
        new_cfg(save_file).unwrap();

        let test_files = vec![
            PathBuf::from("test_files\\test1.txt"),
            PathBuf::from("test_files\\test2.bhd"),
            PathBuf::from("test_files\\test3.dll"),
            PathBuf::from("test_files\\test4.exe"),
            PathBuf::from("test_files\\test5.bin"),
            PathBuf::from("test_files\\config.ini"),
        ];

        let (config_files, files) = split_out_config_files(test_files.clone());
        let test_files_disabled = files
            .iter()
            .map(|file| PathBuf::from(format!("{}.disabled", file.display())))
            .collect::<Vec<_>>();

        let test_mod = RegMod {
            name: String::from("Test"),
            state: true,
            files,
            config_files,
        };
        assert_eq!(test_mod.files.len(), 5);
        assert_eq!(test_mod.config_files.len(), 1);

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
            files: test_files_disabled,
            config_files: test_mod.config_files,
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

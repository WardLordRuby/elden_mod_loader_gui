pub mod common;

#[cfg(test)]
mod tests {
    use elden_mod_loader_gui::{
        does_dir_contain, get_cfg, toggle_files,
        utils::ini::{
            parser::{IniProperty, RegMod},
            writer::{save_path, save_paths},
        },
        Operation, OperationResult, INI_SECTIONS, OFF_STATE,
    };
    use std::{
        fs::{self, remove_file, File},
        path::{Path, PathBuf},
    };

    use crate::common::{file_exists, new_cfg_with_sections, GAME_DIR};

    #[test]
    fn do_files_toggle() {
        let save_file = Path::new("temp\\file_toggle_test.ini");

        let test_files = vec![
            Path::new("test1.txt"),
            Path::new("test2.bhd"),
            Path::new("test3.dll"),
            Path::new("test4.exe"),
            Path::new("test5.bin"),
            Path::new("config.ini"),
        ];
        let test_key = "test_files";
        let prefix_key = "test_dir";
        let prefix = Path::new("temp\\");

        new_cfg_with_sections(save_file, &INI_SECTIONS).unwrap();
        save_path(save_file, INI_SECTIONS[1], prefix_key, prefix).unwrap();
        save_paths(save_file, INI_SECTIONS[3], test_key, &test_files).unwrap();

        let mut test_mod = RegMod::new(
            test_key,
            true,
            test_files.iter().map(PathBuf::from).collect(),
        );
        let mut test_files_disabled = test_mod
            .files
            .dll
            .iter()
            .map(|file| PathBuf::from(format!("{}{OFF_STATE}", file.display())))
            .collect::<Vec<_>>();

        assert_eq!(test_mod.files.dll.len(), 1);
        assert_eq!(test_mod.files.config.len(), 1);
        assert_eq!(test_mod.files.other.len(), 4);

        for test_file in test_files.iter() {
            File::create(test_file).unwrap();
        }

        toggle_files(
            Path::new(""),
            !test_mod.state,
            &mut test_mod,
            Some(save_file),
        )
        .unwrap();

        for path_to_test in test_files_disabled.iter() {
            assert!(file_exists(path_to_test.as_path()));
        }

        test_files_disabled.extend(test_mod.files.config);
        test_files_disabled.extend(test_mod.files.other);

        let read_disabled_ini = IniProperty::<Vec<PathBuf>>::read(
            &get_cfg(save_file).unwrap(),
            INI_SECTIONS[3],
            test_key,
            prefix,
            true,
        )
        .unwrap()
        .value;

        assert!(read_disabled_ini
            .iter()
            .all(|read| test_files_disabled.contains(read)));

        let mut test_mod = RegMod::new(&test_mod.name, false, test_files_disabled);

        toggle_files(
            Path::new(""),
            !test_mod.state,
            &mut test_mod,
            Some(save_file),
        )
        .unwrap();

        for path_to_test in test_files.iter() {
            assert!(file_exists(path_to_test));
        }

        let read_enabled_ini = IniProperty::<Vec<PathBuf>>::read(
            &get_cfg(save_file).unwrap(),
            INI_SECTIONS[3],
            test_key,
            prefix,
            true,
        )
        .unwrap()
        .value;

        assert!(read_enabled_ini
            .iter()
            .all(|read| test_files.contains(&read.as_path())));

        for test_file in test_files.iter() {
            remove_file(test_file).unwrap();
        }
        remove_file(save_file).unwrap();
    }

    #[test]
    #[allow(unused_variables)]
    fn does_dir_contain_work() {
        let mods_dir = PathBuf::from(&format!("{GAME_DIR}\\mods"));
        let entries = fs::read_dir(&mods_dir)
            .unwrap()
            .map(|f| f.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        let num_entries = entries.len();

        assert!(matches!(
            does_dir_contain(
                &mods_dir,
                Operation::Count,
                entries.iter().map(|e| e.as_ref()).collect::<Vec<_>>().as_slice()
            ),
            Ok(OperationResult::Count((num_entries, _)))
        ));

        assert!(matches!(
            does_dir_contain(&mods_dir, Operation::All, &entries),
            Ok(OperationResult::Bool(true))
        ));

        assert!(matches!(
            does_dir_contain(&mods_dir, Operation::Any, &["this_should_not_exist"]),
            Ok(OperationResult::Bool(false))
        ));
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::remove_file,
        path::{Path, PathBuf},
    };

    use elden_mod_loader_gui::{
        get_cgf,
        ini_tools::{parser::IniProperty, writer::*},
    };
    use ini::Ini;

    #[test]
    fn does_path_parse() {
        let test_path =
            Path::new("C:\\Program Files (x86)\\Steam\\steamapps\\common\\ELDEN RING\\Game");
        let test_file = "test_files\\test_path.ini";

        {
            let mut test_ini = Ini::new();
            save_path(
                &mut test_ini,
                test_file,
                Some("paths"),
                "game_dir",
                test_path,
            );
        }

        let config = get_cgf(test_file).unwrap();
        let parse_test = IniProperty::<PathBuf>::read(&config, Some("paths"), "game_dir")
            .unwrap()
            .value;
        assert_eq!(test_path, parse_test);
        remove_file(test_file);
    }

    #[test]
    fn does_array_parse() {
        let input_array = vec![
            PathBuf::from("dinput8.dll"),
            PathBuf::from("movie\\13000050.bk2"),
            PathBuf::from("eldenring.exe"),
            PathBuf::from("EasyAntiCheat\\settings.json"),
        ];
        let test_file = "test_files\\test_array.ini";

        {
            // We must save a working game_dir to the same ini before we can parse Vec<PathBuf>
            // -----------------parser is set up to only parse valid entries------------------
            // use case for parse Vec<PathBuf> is to keep track of files within game_dir
            let mut test_ini = Ini::new();
            let game_path =
                Path::new("C:\\Program Files (x86)\\Steam\\steamapps\\common\\ELDEN RING\\Game");
            save_path(
                &mut test_ini,
                test_file,
                Some("paths"),
                "game_dir",
                game_path,
            );
        }

        {
            let mut test_ini = get_cgf(test_file).unwrap();
            save_path_bufs(&mut test_ini, test_file, "test_files", &input_array);
        }

        let config = get_cgf(test_file).unwrap();
        let parse_test =
            IniProperty::<Vec<PathBuf>>::read(&config, Some("mod-files"), "test_files")
                .unwrap()
                .value;
        assert_eq!(input_array, parse_test);
        remove_file(test_file);
    }

    #[test]
    fn does_bool_parse() {
        let input_bool_0 = false;
        let input_bool_1 = true;
        let test_file = "test_files\\test_bool.ini";

        {
            let mut test_ini = Ini::new();
            save_bool(&mut test_ini, test_file, "test_bool_false", input_bool_0);
            save_bool(&mut test_ini, test_file, "test_bool_true", input_bool_1);
        }

        let config = get_cgf(test_file).unwrap();
        let parse_false =
            IniProperty::<bool>::read(&config, Some("registered-mods"), "test_bool_false")
                .unwrap()
                .value;
        let parse_true =
            IniProperty::<bool>::read(&config, Some("registered-mods"), "test_bool_true")
                .unwrap()
                .value;
        assert_eq!(input_bool_0, parse_false);
        assert_eq!(input_bool_1, parse_true);
        remove_file(test_file);
    }

    #[test]
    fn does_array_delete() {
        let array_to_delete = vec![
            PathBuf::from("dinput8.dll"),
            PathBuf::from("movie\\13000050.bk2"),
            PathBuf::from("eldenring.exe"),
            PathBuf::from("EasyAntiCheat\\settings.json"),
        ];
        let test_file = "test_files\\test_delete_array.ini";

        {
            let mut test_ini = Ini::new();
            let game_path =
                Path::new("C:\\Program Files (x86)\\Steam\\steamapps\\common\\ELDEN RING\\Game");
            save_path(
                &mut test_ini,
                test_file,
                Some("paths"),
                "game_dir",
                game_path,
            );
        }

        {
            let mut test_ini = get_cgf(test_file).unwrap();
            save_path_bufs(
                &mut test_ini,
                test_file,
                "files_to_delete",
                &array_to_delete,
            );
            save_path_bufs(&mut test_ini, test_file, "do_not_delete", &array_to_delete);
        }

        let _ = IniProperty::<PathBuf>::remove_array(test_file, "files_to_delete");
        let test_ini = get_cgf(test_file).unwrap();
        let array_key_deleted = test_ini
            .section(Some("mod-files"))
            .unwrap()
            .contains_key("test_files");
        let other_array = test_ini
            .section(Some("mod-files"))
            .unwrap()
            .contains_key("do_not_delete");

        assert!(!array_key_deleted);
        assert!(other_array);
        remove_file(test_file);
    }
}

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
        let test = IniProperty::<PathBuf>::read(&config, Some("paths"), "game_dir")
            .unwrap()
            .value;
        println!("{:?}", test);
        assert_eq!(test_path, test);
        remove_file(test_file);
    }

    #[test]
    fn does_array_parse() {
        let test_array = vec![
            PathBuf::from("dinput8.dll"),
            PathBuf::from("movie\\13000050.bk2"),
            PathBuf::from("eldenring.exe"),
            PathBuf::from("EasyAntiCheat\\settings.json"),
        ];
        let test_file = "test_files\\test_array.ini";

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
            let mut config = get_cgf(test_file).unwrap();
            save_path_bufs(&mut config, test_file, "test_files", &test_array);
        }

        let config = get_cgf(test_file).unwrap();
        let test = IniProperty::<Vec<PathBuf>>::read(&config, Some("mod-files"), "test_files")
            .unwrap()
            .value;
        assert_eq!(test_array, test);
        remove_file(test_file);
    }

    #[test]
    fn does_bool_parse() {
        let test_bool_0 = false;
        let test_bool_1 = true;
        let test_file = "test_files\\test_bool.ini";

        {
            let mut test_ini = Ini::new();
            save_bool(&mut test_ini, test_file, "test_bool_false", test_bool_0);
            save_bool(&mut test_ini, test_file, "test_bool_true", test_bool_1);
        }
        let config = get_cgf(test_file).unwrap();
        let test_false =
            IniProperty::<bool>::read(&config, Some("registered-mods"), "test_bool_false")
                .unwrap()
                .value;
        let test_true =
            IniProperty::<bool>::read(&config, Some("registered-mods"), "test_bool_true")
                .unwrap()
                .value;
        assert_eq!(test_bool_0, test_false);
        assert_eq!(test_bool_1, test_true);
        remove_file(test_file);
    }
}

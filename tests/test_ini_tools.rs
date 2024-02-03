#[cfg(test)]
mod tests {
    use std::{
        fs::remove_file,
        path::{Path, PathBuf},
    };

    use elden_mod_loader_gui::{
        get_cgf,
        ini_tools::{parser::IniProperty, parser::RegMod, writer::*},
    };
    use ini::Ini;

    #[test]
    fn does_path_parse() {
        let test_path_1 =
            Path::new("C:\\Program Files (x86)\\Steam\\steamapps\\common\\ELDEN RING\\Game");
        let test_path_2 = Path::new("C:\\Windows\\System32");
        let test_file = "test_files\\test_path.ini";
        {
            let mut test_ini = Ini::new();
            save_path(
                &mut test_ini,
                test_file,
                Some("paths"),
                "game_dir",
                test_path_1,
            );
            save_path(
                &mut test_ini,
                test_file,
                Some("paths"),
                "random_dir",
                test_path_2,
            );
        }

        let config = get_cgf(test_file).unwrap();
        let parse_test_1 = IniProperty::<PathBuf>::read(&config, Some("paths"), "game_dir")
            .unwrap()
            .value;
        let parse_test_2 = IniProperty::<PathBuf>::read(&config, Some("paths"), "random_dir")
            .unwrap()
            .value;

        // Tests if paths stored in Section "paths" will parse correctly | these are full length paths
        assert_eq!(test_path_1, parse_test_1);
        assert_eq!(test_path_2, parse_test_2);
        let _ = remove_file(test_file);
    }

    #[test]
    fn read_write_delete_from_ini() {
        let test_file = "test_files\\test_collect_mod_data.ini";
        let mod_1_key = "Unlock The Fps  ";
        let mod_1_state = false;
        let mod_2_key = "Skip The Intro";
        let mod_2_state = true;
        let mod_1 = vec![
            PathBuf::from("mods\\UnlockTheFps.dll"),
            PathBuf::from("mods\\UnlockTheFps\\config.ini"),
        ];
        let mod_2 = PathBuf::from("mods\\SkipTheIntro.dll");

        {
            let _ = new_cfg(test_file);
            let mut test_ini: Ini = get_cgf(test_file).unwrap();

            let invalid_format_1 = vec![
                PathBuf::from("mods\\UnlockTheFps.dll"),
                PathBuf::from("mods\\UnlockTheFps\\config.ini"),
            ];
            let invalid_format_2 = PathBuf::from("mods\\SkipTheIntro.dll");

            // We must save a working game_dir to the same ini before we can parse entries in "mod-files"
            // ---------------------parser is set up to only parse valid entries---------------------
            // use case for entries in section "mod-files" is to keep track of files within game_dir
            let game_path =
                Path::new("C:\\Program Files (x86)\\Steam\\steamapps\\common\\ELDEN RING\\Game");

            save_path_bufs(&mut test_ini, test_file, mod_1_key, &mod_1);
            save_bool(&mut test_ini, test_file, mod_1_key, mod_1_state);
            save_path(
                &mut test_ini,
                test_file,
                Some("mod-files"),
                mod_2_key,
                &mod_2,
            );
            save_bool(&mut test_ini, test_file, mod_2_key, mod_2_state);
            save_path_bufs(
                &mut test_ini,
                test_file,
                "no_matching_state_1",
                &invalid_format_1,
            );
            save_path(
                &mut test_ini,
                test_file,
                Some("mod-files"),
                "no_matching_state_2",
                &invalid_format_2,
            );
            save_bool(&mut test_ini, test_file, "no_matching_path", true);

            save_path(
                &mut test_ini,
                test_file,
                Some("paths"),
                "game_dir",
                game_path,
            );
        }

        // -------------------------------------sync_keys runs from inside RegMod::collect()------------------------------------------------
        // ----this deletes any keys that do not have a matching state eg. (key has state but no files, or key has files but no state)-----
        // this tests delete_entry && delete_array in this case we delete "no_matching_path", "no_matching_state_1", and "no_matching_state_2"
        let registered_mods = RegMod::collect(test_file);
        assert_eq!(registered_mods.len(), 2);

        // Tests name format is correct
        let reg_mod_1: &RegMod = registered_mods
            .iter()
            .find(|data| data.name == mod_1_key.trim())
            .unwrap();
        let reg_mod_2: &RegMod = registered_mods
            .iter()
            .find(|data| data.name == mod_2_key.trim())
            .unwrap();

        // Tests if PathBuf and Vec<PathBuf> was parsed correctly
        assert_eq!(mod_1, reg_mod_1.files);
        assert_eq!(mod_2, reg_mod_2.files[0]);

        // Tests if bool was parsed correctly
        assert_eq!(mod_1_state, reg_mod_1.state);
        assert_eq!(mod_2_state, reg_mod_2.state);

        let _ = remove_file(test_file);
    }
}

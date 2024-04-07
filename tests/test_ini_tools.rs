#[cfg(test)]
mod tests {
    use std::{
        fs::remove_file,
        path::{Path, PathBuf},
    };

    use elden_mod_loader_gui::{
        get_cfg,
        utils::ini::{
            parser::{IniProperty, RegMod, Valitidity},
            writer::*,
        },
    };

    #[test]
    fn does_u32_parse() {
        let test_nums: [u32; 3] = [2342652342, 2343523423, 69420];
        let test_file = Path::new("test_files\\test_nums.ini");

        new_cfg(test_file).unwrap();
        for (i, num) in test_nums.iter().enumerate() {
            save_value_ext(
                test_file,
                Some("paths"),
                &format!("test_num_{i}"),
                &num.to_string(),
            )
            .unwrap();
        }

        let config = get_cfg(test_file).unwrap();

        for (i, _) in test_nums.iter().enumerate() {
            assert_eq!(
                test_nums[i],
                IniProperty::<u32>::read(&config, Some("paths"), &format!("test_num_{i}"), false)
                    .unwrap()
                    .value
            )
        }

        remove_file(test_file).unwrap();
    }

    #[test]
    fn does_path_parse() {
        let test_path_1 =
            Path::new("C:\\Program Files (x86)\\Steam\\steamapps\\common\\ELDEN RING\\Game");
        let test_path_2 = Path::new("C:\\Windows\\System32");
        let test_file = Path::new("test_files\\test_path.ini");

        {
            new_cfg(test_file).unwrap();
            save_path(test_file, Some("paths"), "game_dir", test_path_1).unwrap();
            save_path(test_file, Some("paths"), "random_dir", test_path_2).unwrap();
        }

        let config = get_cfg(test_file).unwrap();
        let parse_test_1 = IniProperty::<PathBuf>::read(&config, Some("paths"), "game_dir", false)
            .unwrap()
            .value;
        let parse_test_2 =
            IniProperty::<PathBuf>::read(&config, Some("paths"), "random_dir", false)
                .unwrap()
                .value;

        // Tests if paths stored in Section("paths") will parse correctly | these are full length paths
        assert_eq!(test_path_1, parse_test_1);
        assert_eq!(test_path_2, parse_test_2);
        remove_file(test_file).unwrap();
    }

    #[test]
    fn read_write_delete_from_ini() {
        let test_file = Path::new("test_files\\test_collect_mod_data.ini");
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
            // Test if new_cfg will write all Sections to the file with .is_setup()
            new_cfg(test_file).unwrap();
            assert!(get_cfg(test_file).unwrap().is_setup());

            let invalid_format_1 = vec![
                PathBuf::from("mods\\UnlockTheFps.dll"),
                PathBuf::from("mods\\UnlockTheFps\\config.ini"),
            ];
            let invalid_format_2 = PathBuf::from("mods\\SkipTheIntro.dll");

            // We must save a working game_dir in the ini before we can parse entries in Section("mod-files")
            // -----------------------parser is set up to only parse valid entries---------------------------
            // ----use case for entries in Section("mod-files") is to keep track of files within game_dir----
            let game_path =
                Path::new("C:\\Program Files (x86)\\Steam\\steamapps\\common\\ELDEN RING\\Game");

            save_path_bufs(test_file, mod_1_key, &mod_1).unwrap();
            save_bool(test_file, Some("registered-mods"), mod_1_key, mod_1_state).unwrap();
            save_path(test_file, Some("mod-files"), mod_2_key, &mod_2).unwrap();
            save_bool(test_file, Some("registered-mods"), mod_2_key, mod_2_state).unwrap();
            save_path_bufs(test_file, "no_matching_state_1", &invalid_format_1).unwrap();
            save_path(
                test_file,
                Some("mod-files"),
                "no_matching_state_2",
                &invalid_format_2,
            )
            .unwrap();
            save_bool(test_file, Some("registered-mods"), "no_matching_path", true).unwrap();

            save_path(test_file, Some("paths"), "game_dir", game_path).unwrap();
        }

        // -------------------------------------sync_keys runs from inside RegMod::collect()------------------------------------------------
        // ----this deletes any keys that do not have a matching state eg. (key has state but no files, or key has files but no state)-----
        // this tests delete_entry && delete_array in this case we delete "no_matching_path", "no_matching_state_1", and "no_matching_state_2"
        let registered_mods = RegMod::collect(test_file, false).unwrap();
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

        // Tests if PathBuf and Vec<PathBuf>'s from Section("mod-files") parse correctly | these are partial paths
        assert_eq!(mod_1[0], reg_mod_1.files[0]);
        assert_eq!(mod_1[1], reg_mod_1.config_files[0]);
        assert_eq!(mod_2, reg_mod_2.files[0]);

        // Tests if bool was parsed correctly
        assert_eq!(mod_1_state, reg_mod_1.state);
        assert_eq!(mod_2_state, reg_mod_2.state);

        remove_file(test_file).unwrap();
    }
}

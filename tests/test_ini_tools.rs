pub mod common;

#[cfg(test)]
mod tests {
    use std::{
        fs::{remove_file, File},
        path::{Path, PathBuf},
    };

    use elden_mod_loader_gui::{
        get_cfg,
        utils::ini::{
            common::*,
            mod_loader::Countable,
            parser::{IniProperty, RegMod, Setup},
            writer::*,
        },
        INI_KEYS, INI_SECTIONS, LOADER_FILES, LOADER_SECTIONS, OFF_STATE,
    };

    use crate::common::{new_cfg_with_sections, GAME_DIR};

    #[test]
    fn does_u32_parse() {
        let test_nums: [u32; 3] = [2342652342, 2343523423, 69420];
        let test_file = Path::new("temp\\test_nums.ini");
        let test_section = [Some("u32s")];

        new_cfg_with_sections(test_file, &test_section).unwrap();
        for (i, num) in test_nums.iter().enumerate() {
            save_value_ext(
                test_file,
                test_section[0],
                &format!("test_num_{i}"),
                &num.to_string(),
            )
            .unwrap();
        }

        let config = get_cfg(test_file).unwrap();

        for (i, num) in test_nums.iter().enumerate() {
            assert_eq!(
                *num,
                IniProperty::<u32>::read(&config, test_section[0], &format!("test_num_{i}"))
                    .unwrap()
                    .value
            )
        }

        remove_file(test_file).unwrap();
    }

    #[test]
    fn does_bool_parse() {
        let test_bools: [&str; 6] = [" True ", "false", "faLSe", "0 ", "0", "1"];
        let bool_results: [bool; 6] = [true, false, false, false, false, true];
        let test_file = Path::new("temp\\test_bools.ini");
        let test_section = [Some("bools")];

        new_cfg_with_sections(test_file, &test_section).unwrap();
        for (i, bool_str) in test_bools.iter().enumerate() {
            save_value_ext(
                test_file,
                test_section[0],
                &format!("test_bool_{i}"),
                bool_str,
            )
            .unwrap();
        }

        let config = get_cfg(test_file).unwrap();

        for (i, bool) in bool_results.iter().enumerate() {
            assert_eq!(
                *bool,
                IniProperty::<bool>::read(&config, test_section[0], &format!("test_bool_{i}"))
                    .unwrap()
                    .value
            )
        }

        remove_file(test_file).unwrap();
    }

    #[test]
    fn does_path_parse() {
        let test_path_1 = Path::new(GAME_DIR);
        let test_path_2 = Path::new("C:\\Windows\\System32");
        let test_file = Path::new("temp\\test_path.ini");
        let test_section = [Some("path")];

        {
            new_cfg_with_sections(test_file, &test_section).unwrap();
            save_path(test_file, test_section[0], INI_KEYS[1], test_path_1).unwrap();
            save_path(test_file, test_section[0], "random_dir", test_path_2).unwrap();
        }

        let config = get_cfg(test_file).unwrap();
        let parse_test_1 =
            IniProperty::<PathBuf>::read(&config, test_section[0], INI_KEYS[1], None, false)
                .unwrap()
                .value;
        let parse_test_2 =
            IniProperty::<PathBuf>::read(&config, test_section[0], "random_dir", None, false)
                .unwrap()
                .value;

        // Tests if paths stored in Section("paths") will parse correctly | these are full length paths
        assert_eq!(test_path_1, parse_test_1);
        assert_eq!(test_path_2, parse_test_2);
        remove_file(test_file).unwrap();
    }

    #[test]
    fn test_sort_by_order() {
        let test_keys = ["a_mod", "b_mod", "c_mod", "d_mod", "f_mod", "e_mod"];
        let test_files = test_keys
            .iter()
            .map(|k| PathBuf::from(format!("{k}.dll")))
            .collect::<Vec<_>>();
        let test_values = ["69420", "2", "1", "0"];
        let sorted_order = ["d_mod", "c_mod", "b_mod", "a_mod", "e_mod", "f_mod"];

        let test_file = PathBuf::from(&format!("temp\\{}", LOADER_FILES[2]));
        let required_file = PathBuf::from(&format!("temp\\{}", LOADER_FILES[1]));

        let test_sections = [LOADER_SECTIONS[0], LOADER_SECTIONS[1], Some("paths")];
        {
            new_cfg_with_sections(&test_file, &test_sections).unwrap();
            for (i, key) in test_keys.iter().enumerate() {
                save_path(&test_file, test_sections[2], key, &test_files[i]).unwrap();
            }
            for (i, value) in test_values.iter().enumerate() {
                save_value_ext(
                    &test_file,
                    test_sections[1],
                    test_files[i].to_str().unwrap(),
                    value,
                )
                .unwrap();
            }
            File::create(&required_file).unwrap();
        }

        let mut cfg = ModLoaderCfg::read(&test_file).unwrap();

        let parsed_cfg = cfg.parse_section().unwrap();

        cfg.update_order_entries(None).unwrap();
        let order = test_keys
            .iter()
            .enumerate()
            .map(|(i, key)| {
                RegMod::with_load_order(key, true, vec![test_files[i].clone()], &parsed_cfg)
            })
            .collect::<Vec<_>>();

        // this tests to make sure the two without an order set are marked as order.set = false
        assert_eq!(order.order_count(), test_values.len());

        // this tests that the order is set correclty for the mods that have a order entry
        order
            .iter()
            .filter(|m| m.order.set)
            .for_each(|m| assert_eq!(m.name, sorted_order[m.order.at]));

        remove_file(test_file).unwrap();
        remove_file(required_file).unwrap();
    }

    #[test]
    #[allow(unused_variables)]
    fn type_check() {
        let test_path = Path::new(GAME_DIR);
        let test_array = [Path::new("temp\\test"), Path::new("temp\\test")];
        let test_file = Path::new("temp\\test_type_check.ini");
        let test_sections = [Some("path"), Some("paths")];
        let array_key = "test_array";

        new_cfg(test_file).unwrap();
        save_path(test_file, test_sections[0], INI_KEYS[1], test_path).unwrap();
        save_paths(test_file, test_sections[1], array_key, &test_array).unwrap();

        let config = get_cfg(test_file).unwrap();

        let pathbuf_err = std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid type found. Expected: Path, Found: Vec<Path>",
        );
        let vec_pathbuf_err = std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid type found. Expected: Vec<Path>, Found: Path",
        );

        let vec_result = IniProperty::<Vec<PathBuf>>::read(
            &config,
            test_sections[0],
            INI_KEYS[1],
            test_path,
            false,
        );
        assert_eq!(
            vec_result.unwrap_err().to_string(),
            vec_pathbuf_err.to_string()
        );

        let path_result =
            IniProperty::<PathBuf>::read(&config, test_sections[1], array_key, None, false);
        assert_eq!(
            path_result.unwrap_err().to_string(),
            pathbuf_err.to_string()
        );

        remove_file(test_file).unwrap();
    }

    #[test]
    fn read_write_delete_from_ini() {
        let test_file = Path::new("temp\\test_collect_mod_data.ini");
        let game_path = Path::new(GAME_DIR);

        let mod_1_files = vec![
            PathBuf::from("mods\\UnlockTheFps.dll"),
            PathBuf::from("mods\\UnlockTheFps\\config.ini"),
        ];
        let mod_2_file = PathBuf::from("mods\\SkipTheIntro.dll");

        // test_mod_2 state is set incorrectly
        let test_mod_1 = RegMod::new("Unlock The Fps  ", true, mod_1_files);
        let mut test_mod_2 = RegMod::new(" Skip The Intro", false, vec![mod_2_file]);

        {
            // Test if new_cfg will write all Sections to the file with .is_setup()
            new_cfg(test_file).unwrap();
            assert!(test_file.is_setup(&INI_SECTIONS).is_ok());

            let invalid_format_1 = vec![
                Path::new("mods\\UnlockTheFps.dll"),
                Path::new("mods\\UnlockTheFps\\config.ini"),
            ];
            let invalid_format_2 = PathBuf::from("mods\\SkipTheIntro.dll");

            // We must save a working game_dir in the ini before we can parse entries in Section("mod-files")
            // -----------------------parser is set up to only parse valid entries---------------------------
            // ----use case for entries in Section("mod-files") is to keep track of files within game_dir----

            save_paths(
                test_file,
                INI_SECTIONS[3],
                &test_mod_1.name,
                &test_mod_1.files.file_refs(),
            )
            .unwrap();
            save_bool(
                test_file,
                INI_SECTIONS[2],
                &test_mod_1.name,
                test_mod_1.state,
            )
            .unwrap();
            save_path(
                test_file,
                INI_SECTIONS[3],
                &test_mod_2.name,
                &test_mod_2.files.dll[0],
            )
            .unwrap();
            save_bool(
                test_file,
                INI_SECTIONS[2],
                &test_mod_2.name,
                test_mod_2.state,
            )
            .unwrap();
            save_paths(
                test_file,
                INI_SECTIONS[3],
                "no_matching_state_1",
                &invalid_format_1,
            )
            .unwrap();
            save_path(
                test_file,
                INI_SECTIONS[3],
                "no_matching_state_2",
                &invalid_format_2,
            )
            .unwrap();
            save_bool(test_file, INI_SECTIONS[2], "no_matching_path", true).unwrap();

            save_path(test_file, INI_SECTIONS[1], INI_KEYS[1], game_path).unwrap();
        }

        // -------------------------------------sync_keys() runs from inside Cfg.collect_mods()------------------------------------------------
        // ----this deletes any keys that do not have a matching state eg. (key has state but no files, or key has files but no state)-----
        // this tests delete_entry && delete_array in this case we delete "no_matching_path", "no_matching_state_1", and "no_matching_state_2"
        let cfg = Cfg::read(test_file).unwrap();
        let mut reg_mods = cfg.collect_mods(game_path, None, false).mods;
        assert_eq!(reg_mods.len(), 2);

        // Tests name format is correct
        let mod_1 = reg_mods
            .iter()
            .position(|data| data.name == test_mod_1.name.trim())
            .unwrap();
        let mod_2 = reg_mods
            .iter()
            .position(|data| data.name == test_mod_2.name.trim())
            .unwrap();

        // verify_state() also runs from within Cfg.collect_mods() lets see if changed the state of the mods .dll file
        let disabled_state = format!("{}{}", test_mod_2.files.dll[0].display(), OFF_STATE);
        assert!(matches!(
            game_path.join(&disabled_state).try_exists(),
            Ok(true)
        ));
        assert_eq!(
            reg_mods[mod_2].files.dll,
            vec![PathBuf::from(disabled_state)]
        );

        // lets set it correctly now
        reg_mods[mod_2].state = true;
        reg_mods[mod_2].verify_state(game_path, test_file).unwrap();
        assert!(matches!(
            game_path.join(&test_mod_2.files.dll[0]).try_exists(),
            Ok(true)
        ));
        assert_eq!(reg_mods[mod_2].files.dll, test_mod_2.files.dll);

        test_mod_2.state = IniProperty::<bool>::read(
            &get_cfg(test_file).unwrap(),
            INI_SECTIONS[2],
            &test_mod_2.name,
        )
        .unwrap()
        .value;

        // Tests if PathBuf and Vec<PathBuf>'s from Section("mod-files") parse correctly | these are partial paths
        assert_eq!(test_mod_1.files.dll[0], reg_mods[mod_1].files.dll[0]);
        assert_eq!(test_mod_1.files.config[0], reg_mods[mod_1].files.config[0]);
        assert_eq!(test_mod_2.files.dll[0], reg_mods[mod_2].files.dll[0]);

        // Tests if bool was parsed correctly
        assert_eq!(test_mod_1.state, reg_mods[mod_1].state);
        assert!(test_mod_2.state);

        remove_file(test_file).unwrap();
    }
}

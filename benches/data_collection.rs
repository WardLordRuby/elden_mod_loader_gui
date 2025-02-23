use criterion::{Criterion, black_box, criterion_group, criterion_main};

use elden_mod_loader_gui::{
    INI_NAME, INI_SECTIONS,
    utils::ini::{
        common::{Cfg, Config},
        writer::{new_cfg, save_bool, save_path, save_paths},
    },
};
use rand::{Rng, distr::Alphanumeric};
use std::{
    fs::remove_file,
    path::{Path, PathBuf},
};

const NUM_ENTRIES: u32 = 25;

fn populate_non_valid_ini(len: u32, file: &Path) {
    new_cfg(file).unwrap();
    for i in 0..len {
        let key = format!("key_{}", i);
        let bool_value = rand::rng().random_bool(0.5);
        let paths = generate_test_paths();
        let path_refs = paths.iter().map(|p| p.as_path()).collect::<Vec<_>>();

        save_bool(file, INI_SECTIONS[2], &key, bool_value).unwrap();
        if paths.len() > 1 {
            save_paths(file, INI_SECTIONS[3], &key, &path_refs).unwrap();
        } else {
            save_path(file, INI_SECTIONS[3], &key, paths[0].as_path()).unwrap();
        }
    }
}

fn generate_test_paths() -> Vec<PathBuf> {
    let num_paths = rand::rng().random_range(1..5);
    (0..num_paths)
        .map(|_| {
            PathBuf::from(
                rand::rng()
                    .sample_iter(&Alphanumeric)
                    .take(10)
                    .map(char::from)
                    .chain(".dll".chars())
                    .collect::<String>(),
            )
        })
        .collect()
}

fn data_collection_benchmark(c: &mut Criterion) {
    // to get around the validation tests we set the name of the test file to the name the _release_ code expects as valid
    let test_file = PathBuf::from(&format!("temp\\{}", INI_NAME));
    let ini = Cfg::read(&test_file).unwrap();
    populate_non_valid_ini(NUM_ENTRIES, &test_file);

    c.bench_function("data_collection", |b| {
        b.iter(|| black_box(ini.collect_mods(Path::new(""), None, true)));
    });
    remove_file(test_file).unwrap();
}

criterion_group!(benches, data_collection_benchmark);
criterion_main!(benches);

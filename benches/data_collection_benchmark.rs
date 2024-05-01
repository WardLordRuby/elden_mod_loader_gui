use criterion::{black_box, criterion_group, criterion_main, Criterion};

use elden_mod_loader_gui::{
    utils::ini::{parser::RegMod, writer::*},
    INI_SECTIONS,
};
use rand::{distributions::Alphanumeric, Rng};
use std::{
    fs::remove_file,
    path::{Path, PathBuf},
};

const BENCH_TEST_FILE: &str = "temp\\benchmark_test.ini";
const NUM_ENTRIES: u32 = 25;

fn populate_non_valid_ini(len: u32, file: &Path) {
    new_cfg(file).unwrap();
    for i in 0..len {
        let key = format!("key_{}", i);
        let bool_value = rand::thread_rng().gen_bool(0.5);
        let paths = generate_test_paths();
        let path_refs = paths.iter().map(|p| p.as_path()).collect::<Vec<_>>();

        save_bool(file, INI_SECTIONS[2], &key, bool_value).unwrap();
        if paths.len() > 1 {
            save_paths(file, &key, &path_refs).unwrap();
        } else {
            save_path(file, INI_SECTIONS[3], &key, paths[0].as_path()).unwrap();
        }
    }
}

fn generate_test_paths() -> Vec<PathBuf> {
    let num_paths = rand::thread_rng().gen_range(1..5);
    (0..num_paths)
        .map(|_| {
            PathBuf::from(
                rand::thread_rng()
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
    let test_file = Path::new(BENCH_TEST_FILE);
    populate_non_valid_ini(NUM_ENTRIES, test_file);

    c.bench_function("data_collection", |b| {
        b.iter(|| black_box(RegMod::collect(test_file, true)));
    });
    remove_file(test_file).unwrap();
}

criterion_group!(benches, data_collection_benchmark);
criterion_main!(benches);

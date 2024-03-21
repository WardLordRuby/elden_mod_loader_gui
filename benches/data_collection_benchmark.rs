use criterion::{black_box, criterion_group, criterion_main, Criterion};

use elden_mod_loader_gui::ini_tools::{parser::RegMod, writer::*};
use rand::{distributions::Alphanumeric, Rng};
use std::{fs::remove_file, path::PathBuf};

fn populate_non_valid_ini(len: u32, file: &str) {
    let _ = new_cfg(file);
    for i in 0..len {
        let key = format!("key_{}", i);
        let bool_value = rand::thread_rng().gen_bool(0.5);
        let paths = generate_test_paths();

        let _ = save_bool(BENCH_TEST_FILE, Some("registered-mods"), &key, bool_value);
        if paths.len() > 1 {
            let _ = save_path_bufs(BENCH_TEST_FILE, &key, &paths);
        } else {
            let _ = save_path(BENCH_TEST_FILE, Some("mod-files"), &key, paths[0].as_path());
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
                    .collect::<String>(),
            )
        })
        .collect()
}

const BENCH_TEST_FILE: &str = "test_files\\benchmark_test.ini";
const NUM_ENTRIES: u32 = 25;

fn data_collection_benchmark(c: &mut Criterion) {
    populate_non_valid_ini(NUM_ENTRIES, BENCH_TEST_FILE);

    c.bench_function("data_collection", |b| {
        b.iter(|| black_box(RegMod::collect(BENCH_TEST_FILE, true)));
    });
    let _ = remove_file(BENCH_TEST_FILE);
}

criterion_group!(benches, data_collection_benchmark);
criterion_main!(benches);

use ini::{EscapePolicy, Ini, LineSeparator, WriteOption};

use std::{
    fs::File,
    io::{self, Write},
    path::{Path, PathBuf},
};

const WRITE_OPTIONS: WriteOption = WriteOption {
    escape_policy: EscapePolicy::Nothing,
    line_separator: LineSeparator::CRLF,
    kv_separator: "=",
};

pub fn save_path_bufs(config: &mut Ini, file_name: &str, key: &str, files: &[PathBuf]) {
    let save_paths = files
        .iter()
        .map(|path| path.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\r\narray[]=");
    config
        .with_section(Some("mod-files"))
        .set(key, format!("array\r\narray[]={}", save_paths));
    config.write_to_file_opt(file_name, WRITE_OPTIONS).unwrap();
}

pub fn save_path(config: &mut Ini, file_name: &str, section: Option<&str>, key: &str, path: &Path) {
    config
        .with_section(section)
        .set(key, path.to_string_lossy().to_string());
    config.write_to_file_opt(file_name, WRITE_OPTIONS).unwrap();
}

pub fn save_bool(config: &mut Ini, file_name: &str, key: &str, value: bool) {
    config
        .with_section(Some("registered-mods"))
        .set(key, value.to_string());
    config.write_to_file_opt(file_name, WRITE_OPTIONS).unwrap();
}

pub fn new_cfg(path: &str) -> io::Result<()> {
    let mut new_ini = File::create(path)?;

    writeln!(new_ini, "[paths]")?;
    writeln!(new_ini, "[registerd-mods]")?;
    writeln!(new_ini, "[mod-files]")?;

    Ok(())
}
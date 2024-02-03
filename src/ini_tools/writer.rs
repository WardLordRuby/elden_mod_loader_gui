use ini::{EscapePolicy, Ini, LineSeparator, WriteOption};

use std::{
    fs::{read_to_string, write, File},
    io::{self, Write},
    path::{Path, PathBuf},
};

const WRITE_OPTIONS: WriteOption = WriteOption {
    escape_policy: EscapePolicy::Nothing,
    line_separator: LineSeparator::CRLF,
    kv_separator: "=",
};

pub fn save_path_bufs(config: &mut Ini, file_name: &str, key: &str, files: &[PathBuf]) {
    let format_key = key.trim().replace(' ', "_");
    let save_paths = files
        .iter()
        .map(|path| path.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\r\narray[]=");
    config
        .with_section(Some("mod-files"))
        .set(&format_key, format!("array\r\narray[]={}", save_paths));
    config.write_to_file_opt(file_name, WRITE_OPTIONS).unwrap();
}

pub fn save_path(config: &mut Ini, file_name: &str, section: Option<&str>, key: &str, path: &Path) {
    let format_key = key.trim().replace(' ', "_");
    config
        .with_section(section)
        .set(&format_key, path.to_string_lossy().to_string());
    config.write_to_file_opt(file_name, WRITE_OPTIONS).unwrap();
}

pub fn save_bool(config: &mut Ini, file_name: &str, key: &str, value: bool) {
    let format_key = key.trim().replace(' ', "_");
    config
        .with_section(Some("registered-mods"))
        .set(&format_key, value.to_string());
    config.write_to_file_opt(file_name, WRITE_OPTIONS).unwrap();
}

pub fn new_cfg(path: &str) -> io::Result<()> {
    let mut new_ini = File::create(path)?;

    writeln!(new_ini, "[paths]")?;
    writeln!(new_ini, "[registered-mods]")?;
    writeln!(new_ini, "[mod-files]")?;

    Ok(())
}

pub fn remove_array(path: &str, key: &str) -> io::Result<()> {
    let content = read_to_string(path)?;

    let mut skip_next_line = false;
    let mut key_found = false;

    let mut filter_lines = |line: &str| {
        if key_found && !line.starts_with("array[]") {
            skip_next_line = false;
            key_found = false;
        }
        if line.starts_with(key) {
            skip_next_line = true;
            key_found = true;
        }
        !skip_next_line
    };

    let lines: Vec<&str> = content.lines().filter(|&line| filter_lines(line)).collect();

    write(path, lines.join("\r\n"))?;
    Ok(())
}

pub fn remove_entry(
    config: &mut Ini,
    path: &str,
    section: Option<&str>,
    key: &str,
) -> io::Result<()> {
    config.delete_from(section, key);
    config.write_to_file_opt(path, WRITE_OPTIONS)
}

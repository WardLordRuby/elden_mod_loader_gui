use ini::{EscapePolicy, Ini, LineSeparator, WriteOption};
use log::{error, trace};

use std::{
    fs::{read_to_string, write, File},
    io::{self, Write},
    path::{Path, PathBuf},
};

use crate::get_cfg;

const WRITE_OPTIONS: WriteOption = WriteOption {
    escape_policy: EscapePolicy::Nothing,
    line_separator: LineSeparator::CRLF,
    kv_separator: "=",
};

pub fn save_path_bufs(file_name: &str, key: &str, files: &[PathBuf]) -> io::Result<()> {
    let mut config: Ini = match get_cfg(file_name) {
        Ok(ini) => {
            trace!("Success: (save_path_bufs) Read ini from \"{}\"", file_name);
            ini
        }
        Err(err) => {
            error!(
                "Failure: (save_path_bufs) Could not complete. Could not read ini from \"{}\"",
                file_name
            );
            error!("Error: {}", err);
            return Ok(());
        }
    };
    let format_key = key.trim().replace(' ', "_");
    let save_paths = files
        .iter()
        .map(|path| path.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\r\narray[]=");
    config
        .with_section(Some("mod-files"))
        .set(&format_key, format!("array\r\narray[]={}", save_paths));
    config.write_to_file_opt(file_name, WRITE_OPTIONS)
}

pub fn save_path(file_name: &str, section: Option<&str>, key: &str, path: &Path) -> io::Result<()> {
    let mut config: Ini = match get_cfg(file_name) {
        Ok(ini) => {
            trace!("Success: (save_path) Read ini from \"{}\"", file_name);
            ini
        }
        Err(err) => {
            error!(
                "Failure: (save_path) Could not complete. Could not read ini from \"{}\"",
                file_name
            );
            error!("Error: {}", err);
            return Ok(());
        }
    };
    let format_key = key.trim().replace(' ', "_");
    config
        .with_section(section)
        .set(&format_key, path.to_string_lossy().to_string());
    config.write_to_file_opt(file_name, WRITE_OPTIONS)
}

pub fn save_bool(file_name: &str, key: &str, value: bool) -> io::Result<()> {
    let mut config: Ini = match get_cfg(file_name) {
        Ok(ini) => {
            trace!("Success: (save_bool) Read ini from \"{}\"", file_name);
            ini
        }
        Err(err) => {
            error!(
                "Failure: (save_bool) Could not complete. Could not read ini from \"{}\"",
                file_name
            );
            error!("Error: {}", err);
            return Ok(());
        }
    };
    let format_key = key.trim().replace(' ', "_");
    config
        .with_section(Some("registered-mods"))
        .set(&format_key, value.to_string());
    config.write_to_file_opt(file_name, WRITE_OPTIONS)
}

pub fn new_cfg(path: &str) -> io::Result<()> {
    let mut new_ini = File::create(path)?;

    writeln!(new_ini, "[paths]")?;
    writeln!(new_ini, "[registered-mods]")?;
    writeln!(new_ini, "[mod-files]")?;

    Ok(())
}

pub fn remove_array(file_name: &str, key: &str) -> io::Result<()> {
    let format_key = key.trim().replace(' ', "_");
    let content = read_to_string(file_name)?;

    let mut skip_next_line = false;
    let mut key_found = false;

    let mut filter_lines = |line: &str| {
        if key_found && !line.starts_with("array[]") {
            skip_next_line = false;
            key_found = false;
        }
        if line.starts_with(&format_key) {
            skip_next_line = true;
            key_found = true;
        }
        !skip_next_line
    };

    let lines: Vec<&str> = content.lines().filter(|&line| filter_lines(line)).collect();

    write(file_name, lines.join("\r\n"))
}

pub fn remove_entry(file_name: &str, section: Option<&str>, key: &str) -> io::Result<()> {
    let mut config: Ini = match get_cfg(file_name) {
        Ok(ini) => {
            trace!("Success: (remove_entry) Read ini from \"{}\"", file_name);
            ini
        }
        Err(err) => {
            error!(
                "Failure: (remove_entry) Could not complete. Could not read ini from \"{}\"",
                file_name
            );
            error!("Error: {}", err);
            return Ok(());
        }
    };
    let format_key = key.trim().replace(' ', "_");
    config.delete_from(section, &format_key);
    config.write_to_file_opt(file_name, WRITE_OPTIONS)
}

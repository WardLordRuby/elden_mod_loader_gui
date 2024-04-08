use ini::{EscapePolicy, Ini, LineSeparator, WriteOption};

use std::{
    fs::{self, read_to_string, write, File},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use crate::get_cfg;

pub const INI_SECTIONS: [&str; 4] = [
    "[app-settings]",
    "[paths]",
    "[registered-mods]",
    "[mod-files]",
];

const WRITE_OPTIONS: WriteOption = WriteOption {
    escape_policy: EscapePolicy::Nothing,
    line_separator: LineSeparator::CRLF,
    kv_separator: "=",
};

const EXT_OPTIONS: WriteOption = WriteOption {
    escape_policy: EscapePolicy::Nothing,
    line_separator: LineSeparator::CRLF,
    kv_separator: " = ",
};

macro_rules! new_io_error {
    ($kind:expr, $msg:expr) => {
        ini::Error::Io(std::io::Error::new($kind, $msg))
    };
}

pub fn save_path_bufs(file_name: &Path, key: &str, files: &[PathBuf]) -> Result<(), ini::Error> {
    let mut config: Ini = get_cfg(file_name)?;
    let save_paths = files
        .iter()
        .map(|path| path.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\r\narray[]=");
    config
        .with_section(Some("mod-files"))
        .set(key, format!("array\r\narray[]={save_paths}"));
    config
        .write_to_file_opt(file_name, WRITE_OPTIONS)
        .map_err(ini::Error::Io)
}

pub fn save_path(
    file_name: &Path,
    section: Option<&str>,
    key: &str,
    path: &Path,
) -> Result<(), ini::Error> {
    let mut config: Ini = get_cfg(file_name)?;
    config
        .with_section(section)
        .set(key, path.to_string_lossy().to_string());
    config
        .write_to_file_opt(file_name, WRITE_OPTIONS)
        .map_err(ini::Error::Io)
}

pub fn save_bool(
    file_name: &Path,
    section: Option<&str>,
    key: &str,
    value: bool,
) -> Result<(), ini::Error> {
    let mut config: Ini = get_cfg(file_name)?;
    config.with_section(section).set(key, value.to_string());
    config
        .write_to_file_opt(file_name, WRITE_OPTIONS)
        .map_err(ini::Error::Io)
}

pub fn save_value_ext(
    file_name: &Path,
    section: Option<&str>,
    key: &str,
    value: &str,
) -> Result<(), ini::Error> {
    let mut config: Ini = get_cfg(file_name)?;
    config.with_section(section).set(key, value);
    config
        .write_to_file_opt(file_name, EXT_OPTIONS)
        .map_err(ini::Error::Io)
}

pub fn new_cfg(path: &Path) -> Result<(), ini::Error> {
    let parent = match path.parent() {
        Some(parent) => parent,
        None => {
            return Err(new_io_error!(
                ErrorKind::InvalidData,
                format!("Could not create a parent_dir of \"{}\"", path.display())
            ))
        }
    };
    fs::create_dir_all(parent)?;
    let mut new_ini = File::create(path)?;

    for section in INI_SECTIONS {
        writeln!(new_ini, "{section}")?;
    }

    Ok(())
}

pub fn remove_array(file_name: &Path, key: &str) -> Result<(), ini::Error> {
    let content = read_to_string(file_name)?;

    let mut skip_next_line = false;
    let mut key_found = false;

    let mut filter_lines = |line: &str| {
        if key_found && !line.starts_with("array[]") {
            skip_next_line = false;
            key_found = false;
        }
        if line.starts_with(key) && line.ends_with("array") {
            skip_next_line = true;
            key_found = true;
        }
        !skip_next_line
    };

    let lines = content
        .lines()
        .filter(|&line| filter_lines(line))
        .collect::<Vec<_>>();

    write(file_name, lines.join("\r\n")).map_err(ini::Error::Io)
}

pub fn remove_entry(file_name: &Path, section: Option<&str>, key: &str) -> Result<(), ini::Error> {
    let mut config: Ini = get_cfg(file_name)?;
    config.delete_from(section, key).ok_or(new_io_error!(
        ErrorKind::Other,
        format!(
            "Could not delete \"{key}\" from Section: \"{}\"",
            &section.expect("Passed in section should be valid")
        )
    ))?;
    config
        .write_to_file_opt(file_name, WRITE_OPTIONS)
        .map_err(ini::Error::Io)
}

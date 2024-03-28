use ini::{EscapePolicy, Ini, LineSeparator, WriteOption};

use std::{
    fs::{self, read_to_string, write, File},
    io::{self, Write},
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

pub fn save_path_bufs(file_name: &Path, key: &str, files: &[PathBuf]) -> Result<(), ini::Error> {
    let mut config: Ini = get_cfg(file_name)?;
    let format_key = key.trim().replace(' ', "_");
    let save_paths = files
        .iter()
        .map(|path| path.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\r\narray[]=");
    config
        .with_section(Some("mod-files"))
        .set(&format_key, format!("array\r\narray[]={}", save_paths));
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
    let format_key = key.trim().replace(' ', "_");
    config
        .with_section(section)
        .set(&format_key, path.to_string_lossy().to_string());
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
    let format_key = key.trim().replace(' ', "_");
    config
        .with_section(section)
        .set(&format_key, value.to_string());
    config
        .write_to_file_opt(file_name, WRITE_OPTIONS)
        .map_err(ini::Error::Io)
}

pub fn new_cfg(path: &Path) -> Result<(), ini::Error> {
    if path.components().count() > 1 {
        let mut ancestors = Vec::new();
        for ancestor in path.ancestors() {
            if ancestor != Path::new("") && ancestor.extension().is_none() {
                ancestors.push(ancestor)
            }
        }
        ancestors.reverse();
        for ancestor in ancestors {
            match ancestor.try_exists() {
                Ok(bool) => match bool {
                    true => (),
                    false => fs::create_dir(ancestor).unwrap_or_default(),
                },
                Err(_) => {
                    return Err(ini::Error::Io(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "Permission Denied when trying to access directory",
                    )))
                }
            }
        }
    }
    let mut new_ini = File::create(path)?;

    for section in INI_SECTIONS {
        writeln!(new_ini, "{}", section)?;
    }

    Ok(())
}

pub fn remove_array(file_name: &Path, key: &str) -> Result<(), ini::Error> {
    let format_key = key.trim().replace(' ', "_");
    let content = read_to_string(file_name)?;

    let mut skip_next_line = false;
    let mut key_found = false;

    let mut filter_lines = |line: &str| {
        if key_found && !line.starts_with("array[]") {
            skip_next_line = false;
            key_found = false;
        }
        if line.starts_with(&format_key) && line.ends_with("array") {
            skip_next_line = true;
            key_found = true;
        }
        !skip_next_line
    };

    let lines: Vec<&str> = content.lines().filter(|&line| filter_lines(line)).collect();

    write(file_name, lines.join("\r\n")).map_err(ini::Error::Io)
}

pub fn remove_entry(file_name: &Path, section: Option<&str>, key: &str) -> Result<(), ini::Error> {
    let mut config: Ini = get_cfg(file_name)?;
    let format_key = key.trim().replace(' ', "_");
    config
        .delete_from(section, &format_key)
        .ok_or(ini::Error::Io(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "Could not delete \"{}\" from Section: \"{}\"",
                &format_key,
                &section.expect("Passed in section should be valid")
            ),
        )))?;
    config
        .write_to_file_opt(file_name, WRITE_OPTIONS)
        .map_err(ini::Error::Io)
}

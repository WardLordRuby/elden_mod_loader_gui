use ini::{EscapePolicy, Ini, LineSeparator, WriteOption};
use tracing::{instrument, trace};

use std::{
    fs::{self, read_to_string, write, File},
    io::{ErrorKind, Write},
    path::Path,
};

use crate::{
    file_name_or_err, get_cfg, parent_or_err, utils::ini::parser::RegMod, ARRAY_KEY, ARRAY_VALUE,
    DEFAULT_INI_VALUES, DEFAULT_LOADER_VALUES, INI_KEYS, INI_SECTIONS, LOADER_FILES, LOADER_KEYS,
    LOADER_SECTIONS, OFF_STATE,
};

pub const WRITE_OPTIONS: WriteOption = WriteOption {
    escape_policy: EscapePolicy::Nothing,
    line_separator: LineSeparator::CRLF,
    kv_separator: "=",
};

pub const EXT_OPTIONS: WriteOption = WriteOption {
    escape_policy: EscapePolicy::Nothing,
    line_separator: LineSeparator::CRLF,
    kv_separator: " = ",
};

pub fn save_paths<P: AsRef<Path>>(
    file_path: &Path,
    section: Option<&str>,
    key: &str,
    files: &[P],
) -> std::io::Result<()> {
    let mut config: Ini = get_cfg(file_path)?;
    let save_paths = files
        .iter()
        .map(|path| path.as_ref().to_string_lossy())
        .collect::<Vec<_>>()
        .join(&format!("\r\n{ARRAY_KEY}="));
    config
        .with_section(section)
        .set(key, format!("{ARRAY_VALUE}\r\n{ARRAY_KEY}={save_paths}"));
    config.write_to_file_opt(file_path, WRITE_OPTIONS)
}

pub fn save_path(
    file_path: &Path,
    section: Option<&str>,
    key: &str,
    path: &Path,
) -> std::io::Result<()> {
    let mut config: Ini = get_cfg(file_path)?;
    config
        .with_section(section)
        .set(key, path.to_string_lossy().to_string());
    config.write_to_file_opt(file_path, WRITE_OPTIONS)
}

pub fn save_bool(
    file_path: &Path,
    section: Option<&str>,
    key: &str,
    value: bool,
) -> std::io::Result<()> {
    let mut config: Ini = get_cfg(file_path)?;
    config.with_section(section).set(key, value.to_string());
    config.write_to_file_opt(file_path, WRITE_OPTIONS)
}

pub fn save_value_ext(
    file_path: &Path,
    section: Option<&str>,
    key: &str,
    value: &str,
) -> std::io::Result<()> {
    let mut config: Ini = get_cfg(file_path)?;
    config.with_section(section).set(key, value);
    config.write_to_file_opt(file_path, EXT_OPTIONS)
}

#[instrument(level = "trace", skip_all)]
pub fn new_cfg(path: &Path) -> std::io::Result<Ini> {
    let file_name = file_name_or_err(path)?;
    let parent = parent_or_err(path)?;

    fs::create_dir_all(parent)?;
    let mut new_ini = File::create(path)?;
    trace!(?file_name, "created on disk");

    if file_name == LOADER_FILES[2] {
        for (i, section) in LOADER_SECTIONS.iter().enumerate() {
            writeln!(new_ini, "[{}]", section.unwrap())?;
            if i == 0 {
                for (j, _) in LOADER_KEYS.iter().enumerate() {
                    writeln!(new_ini, "{} = {}", LOADER_KEYS[j], DEFAULT_LOADER_VALUES[j])?
                }
            }
        }
    } else {
        for (i, section) in INI_SECTIONS.iter().enumerate() {
            writeln!(new_ini, "[{}]", section.unwrap())?;
            if i == 0 {
                writeln!(new_ini, "{}={}", INI_KEYS[i], DEFAULT_INI_VALUES[i])?
            }
        }
    }
    trace!("default sections wrote to file");
    get_cfg(path)
}

pub fn remove_array(file_path: &Path, key: &str) -> std::io::Result<()> {
    let content = read_to_string(file_path)?;

    let mut skip_next_line = false;
    let mut key_found = false;

    let mut filter_lines = |line: &str| {
        if key_found && !line.starts_with(ARRAY_KEY) {
            skip_next_line = false;
            key_found = false;
        }
        if line.starts_with(key) && line.ends_with(ARRAY_VALUE) {
            skip_next_line = true;
            key_found = true;
        }
        !skip_next_line
    };

    let lines = content.lines().filter(|&line| filter_lines(line)).collect::<Vec<_>>();

    write(file_path, lines.join("\r\n"))
}

pub fn remove_entry(file_path: &Path, section: Option<&str>, key: &str) -> std::io::Result<()> {
    let mut config: Ini = get_cfg(file_path)?;
    config.delete_from(section, key).ok_or(std::io::Error::new(
        ErrorKind::Other,
        format!(
            "Could not delete \"{key}\" from Section: \"{}\"",
            &section.expect("Passed in section should be valid")
        ),
    ))?;
    config.write_to_file_opt(file_path, WRITE_OPTIONS)
}

pub fn remove_order_entry(entry: &RegMod, loader_dir: &Path) -> std::io::Result<()> {
    if !entry.order.set {
        return new_io_error!(
            ErrorKind::InvalidInput,
            format!("{} has no order data", entry.name)
        );
    }
    let file_name = file_name_or_err(&entry.files.dll[entry.order.i])?;
    let file_name = file_name.to_str().ok_or(std::io::Error::new(
        ErrorKind::InvalidData,
        format!("{file_name:?} is not valid UTF-8"),
    ))?;
    remove_entry(loader_dir, LOADER_SECTIONS[1], omit_off_state(file_name))?;
    trace!("removed order entry");
    Ok(())
}

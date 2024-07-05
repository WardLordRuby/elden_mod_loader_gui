use ini::{EscapePolicy, Ini, LineSeparator, WriteOption};
use tracing::{info, instrument, trace};

use std::{
    fmt::Display,
    fs::{self, read_to_string, write, File},
    io::{Error, ErrorKind, Result, Write},
    path::Path,
};

use crate::{
    file_name_or_err, get_cfg, new_io_error, omit_off_state, parent_or_err,
    utils::ini::parser::RegMod, DisplayName, ARRAY_KEY, ARRAY_VALUE, DEFAULT_INI_VALUES,
    DEFAULT_LOADER_VALUES, INI_KEYS, INI_NAME, INI_SECTIONS, LOADER_FILES, LOADER_KEYS,
    LOADER_SECTIONS,
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

#[instrument(level = "trace", skip(file_path, section, files), fields(section = section.unwrap()))]
pub fn save_paths<P: AsRef<Path>>(
    file_path: &Path,
    section: Option<&str>,
    key: &str,
    files: &[P],
) -> Result<()> {
    let mut config: Ini = get_cfg(file_path)?;
    let save_paths = files
        .iter()
        .map(|path| path.as_ref().to_string_lossy())
        .collect::<Vec<_>>()
        .join(&format!("\r\n{ARRAY_KEY}="));
    config
        .with_section(section)
        .set(key, format!("{ARRAY_VALUE}\r\n{ARRAY_KEY}={save_paths}"));
    config.write_to_file_opt(file_path, WRITE_OPTIONS)?;
    trace!("saved paths to file");
    Ok(())
}

#[instrument(level = "trace", skip(file_path, section, path), fields(section = section.unwrap()))]
pub fn save_path(file_path: &Path, section: Option<&str>, key: &str, path: &Path) -> Result<()> {
    let mut config: Ini = get_cfg(file_path)?;
    config
        .with_section(section)
        .set(key, path.to_string_lossy().to_string());
    config.write_to_file_opt(file_path, WRITE_OPTIONS)?;
    trace!("saved path to file");
    if let Some(span) = tracing::Span::current().metadata() {
        if key == INI_KEYS[2] && span.name() != "scan_for_mods" {
            info!("Game directory saved as: '{}'", path.display());
        }
    }
    Ok(())
}

#[instrument(level = "trace", skip(file_path, section), fields(section = section.unwrap()))]
pub fn save_bool(file_path: &Path, section: Option<&str>, key: &str, value: bool) -> Result<()> {
    let mut config: Ini = get_cfg(file_path)?;
    config.with_section(section).set(key, value.to_string());
    config.write_to_file_opt(file_path, WRITE_OPTIONS)?;
    trace!("saved bool to file");
    Ok(())
}

#[instrument(level = "trace", skip(file_path, section), fields(section = section.unwrap()))]
pub fn save_value_ext(
    file_path: &Path,
    section: Option<&str>,
    key: &str,
    value: &str,
) -> Result<()> {
    let mut config: Ini = get_cfg(file_path)?;
    config.with_section(section).set(key, value);
    config.write_to_file_opt(file_path, EXT_OPTIONS)?;
    trace!("saved value to file");
    Ok(())
}

fn init_default_values<K, V>(
    writer: &mut File,
    sections: &[Option<&str>],
    keys: &[K],
    values: &[V],
    write_options: WriteOption,
) -> Result<()>
where
    K: Display,
    V: Display,
{
    for (i, section) in sections.iter().enumerate() {
        writeln!(writer, "[{}]", section.expect("section is always some"))?;
        if i == 0 {
            for j in 0..values.len() {
                writeln!(
                    writer,
                    "{}{}{}",
                    &keys[j], write_options.kv_separator, &values[j]
                )?
            }
        }
    }
    Ok(())
}

#[instrument(level = "trace", skip_all, fields(path = %path.display()))]
pub fn new_cfg(path: &Path) -> Result<Ini> {
    let file_name = file_name_or_err(path)?;
    let parent = parent_or_err(path)?;

    fs::create_dir_all(parent)?;
    let mut new_ini = File::create(path)?;

    match file_name {
        f_name if f_name == INI_NAME => {
            init_default_values(
                &mut new_ini,
                &INI_SECTIONS,
                &INI_KEYS,
                &DEFAULT_INI_VALUES,
                WRITE_OPTIONS,
            )?;
            info!("Created new ini: {}", INI_NAME);
        }
        f_name if f_name == LOADER_FILES[3] => {
            init_default_values(
                &mut new_ini,
                &LOADER_SECTIONS,
                &LOADER_KEYS,
                &DEFAULT_LOADER_VALUES,
                EXT_OPTIONS,
            )?;
            info!("Created new ini: {}", LOADER_FILES[3]);
        }
        _ => panic!("No default data implemented for: {file_name:?}"),
    }
    get_cfg(path)
}

#[instrument(level = "trace", skip(file_path))]
pub fn remove_array(file_path: &Path, key: &str) -> Result<()> {
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

    write(file_path, lines.join("\r\n"))?;
    trace!("removed paths from file");
    Ok(())
}

#[instrument(level = "trace", skip(file_path), fields(section = section.unwrap()))]
pub fn remove_entry(file_path: &Path, section: Option<&str>, key: &str) -> Result<()> {
    let mut config: Ini = get_cfg(file_path)?;
    config.delete_from(section, key).ok_or(Error::other(format!(
        "Could not delete: {key}, from Section: {}",
        &section.expect("Passed in section should be valid")
    )))?;
    config.write_to_file_opt(file_path, WRITE_OPTIONS)?;
    trace!("removed entry from file");
    Ok(())
}

#[instrument(level = "trace", skip(loader_dir), fields(mod_name = entry.name))]
pub fn remove_order_entry(entry: &RegMod, loader_dir: &Path) -> Result<()> {
    if !entry.order.set {
        return new_io_error!(
            ErrorKind::InvalidInput,
            format!("{} has no order data", DisplayName(&entry.name))
        );
    }
    let file_name = file_name_or_err(&entry.files.dll[entry.order.i])?;
    let file_name = file_name.to_str().ok_or(Error::new(
        ErrorKind::InvalidData,
        format!("{file_name:?} is not valid UTF-8"),
    ))?;
    remove_entry(loader_dir, LOADER_SECTIONS[1], omit_off_state(file_name))?;
    trace!("removed order entry");
    Ok(())
}

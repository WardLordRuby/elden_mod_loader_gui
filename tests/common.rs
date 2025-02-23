use std::{
    fs::{File, create_dir_all, metadata},
    io::Write,
    path::Path,
};

pub const GAME_DIR: &str = "G:/SteamLibrary/steamapps/common/ELDEN RING/Game";

pub fn new_cfg_with_sections(path: &Path, sections: &[Option<&str>]) -> std::io::Result<()> {
    let parent = path.parent().unwrap();

    create_dir_all(parent)?;
    let mut new_ini = File::create(path)?;

    for section in sections.iter() {
        writeln!(new_ini, "[{}]", section.unwrap())?;
    }
    Ok(())
}

pub fn file_exists(file_path: &Path) -> bool {
    if let Ok(metadata) = metadata(file_path) {
        metadata.is_file()
    } else {
        false
    }
}

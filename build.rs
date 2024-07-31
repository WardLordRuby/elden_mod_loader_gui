#[cfg(target_os = "windows")]
extern crate winresource;

/// `MAJOR << 48 | MINOR << 32 | PATCH << 16 | RELEASE`
const MAJOR: u64 = 0;
const MINOR: u64 = 9;
const PATCH: u64 = 7;
const RELEASE: u64 = 2;

fn main() {
    if cfg!(target_os = "windows") {
        slint_build::compile("ui/appwindow.slint").unwrap();
        let mut res = winresource::WindowsResource::new();
        res.set_icon("ui/assets/EML-icon.ico");
        let version: u64 = (MAJOR << 48) | (MINOR << 32) | (PATCH << 16) | RELEASE;
        res.set_version_info(winresource::VersionInfo::FILEVERSION, version);
        res.compile().unwrap();
    }
}

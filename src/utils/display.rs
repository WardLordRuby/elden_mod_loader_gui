use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};

use crate::utils::ini::parser::LoadOrder;

pub fn format_panic_info(info: &std::panic::PanicInfo) -> String {
    let payload_str = if let Some(location) = info.location() {
        format!(
            "PANIC {}:{}:{}:",
            location.file(),
            location.line(),
            location.column(),
        )
    } else {
        String::from("PANIC:")
    };
    if let Some(msg) = info.payload().downcast_ref::<&str>() {
        format!("{payload_str} {msg}")
    } else if let Some(msg) = info.payload().downcast_ref::<String>() {
        format!("{payload_str} {msg}")
    } else {
        format!("{payload_str} no attached message")
    }
}

pub trait DisplayItem {
    fn display_item(&self, f: &mut std::fmt::Formatter, add: &str) -> std::fmt::Result;
}

impl DisplayItem for &str {
    #[inline]
    fn display_item(&self, f: &mut std::fmt::Formatter, add: &str) -> std::fmt::Result {
        write!(f, "{}{}", self, add)
    }
}

impl DisplayItem for String {
    #[inline]
    fn display_item(&self, f: &mut std::fmt::Formatter, add: &str) -> std::fmt::Result {
        write!(f, "{}{}", self, add)
    }
}

impl DisplayItem for &Path {
    #[inline]
    fn display_item(&self, f: &mut std::fmt::Formatter, add: &str) -> std::fmt::Result {
        write!(f, "{}{}", self.display(), add)
    }
}

impl DisplayItem for PathBuf {
    #[inline]
    fn display_item(&self, f: &mut std::fmt::Formatter, add: &str) -> std::fmt::Result {
        write!(f, "{}{}", self.display(), add)
    }
}

impl DisplayItem for usize {
    #[inline]
    fn display_item(&self, f: &mut std::fmt::Formatter, add: &str) -> std::fmt::Result {
        write!(f, "{}{}", self, add)
    }
}

pub struct DisplayVec<'a, T: DisplayItem>(pub &'a [T]);

impl<'a, T: DisplayItem> std::fmt::Display for DisplayVec<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if self.0.is_empty() {
            panic!("Tried to format an empty Vec");
        }
        if self.0.len() == 1 {
            return self.0[0].display_item(f, "");
        }
        write!(f, "[")?;
        let last_e = self.0.len() - 1;
        self.0.iter().enumerate().try_for_each(|(i, e)| {
            if i != last_e {
                e.display_item(f, ", ")
            } else {
                e.display_item(f, "]")
            }
        })
    }
}

pub struct DisplayMissingOrd<'a>(pub &'a [usize]);

impl<'a> std::fmt::Display for DisplayMissingOrd<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Load order values above: {}, shifted down",
            DisplayVec(self.0)
        )
    }
}

pub struct DisplayName<'a>(pub &'a str);

impl<'a> std::fmt::Display for DisplayName<'a> {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.replace('_', " "))
    }
}

pub struct DisplayState(pub bool);

impl std::fmt::Display for DisplayState {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", if self.0 { "enabled" } else { "disabled" })
    }
}

impl std::fmt::Display for LoadOrder {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.set {
            write!(f, "{}", self.at)
        } else {
            write!(f, "not set")
        }
    }
}

pub struct DisplayTheme(pub bool);

impl std::fmt::Display for DisplayTheme {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", if self.0 { "Dark" } else { "Light" })
    }
}

pub trait IntoIoError {
    fn into_io_error(self, key: &str, context: &str) -> std::io::Error;
}

impl IntoIoError for ini::Error {
    /// converts `ini::Error` into `io::Error` key and context are not used  
    fn into_io_error(self, _key: &str, _context: &str) -> std::io::Error {
        match self {
            ini::Error::Io(err) => err,
            ini::Error::Parse(err) => std::io::Error::new(ErrorKind::InvalidData, err),
        }
    }
}

impl IntoIoError for std::str::ParseBoolError {
    /// converts `ParseBoolError` into `io::Error` key and context add context to err msg
    #[inline]
    fn into_io_error(self, key: &str, context: &str) -> std::io::Error {
        std::io::Error::new(
            ErrorKind::InvalidData,
            format!(
                "string: \"{context}\" found in \"{key}\" was not `true`, `false`, `1`, or `0`."
            ),
        )
    }
}

impl IntoIoError for std::num::ParseIntError {
    /// converts `ParseIntError` into `io::Error` key and context add context to err msg
    #[inline]
    fn into_io_error(self, key: &str, context: &str) -> std::io::Error {
        std::io::Error::new(
            ErrorKind::InvalidData,
            format!(
                "string: \"{context}\" found in \"{key}\" was not within the valid `U32 range`."
            ),
        )
    }
}

pub trait ModError {
    /// replaces self with `self` + `msg`
    fn add_msg(&mut self, msg: &str, add_new_line: bool);
}

impl ModError for std::io::Error {
    #[inline]
    fn add_msg(&mut self, msg: &str, add_new_line: bool) {
        let formatter = if add_new_line { "\n" } else { ", " };
        std::mem::swap(
            self,
            &mut std::io::Error::new(self.kind(), format!("{self}{formatter}{msg}")),
        )
    }
}

pub trait ErrorClone {
    /// clones a immutable reference to an `Error` to a owned `io::Error`
    fn clone_err(&self) -> std::io::Error;
}

impl ErrorClone for std::io::Error {
    #[inline]
    fn clone_err(&self) -> std::io::Error {
        std::io::Error::new(self.kind(), self.to_string())
    }
}

pub trait Merge {
    /// joins all `io::Error`'s in a collection while leaving the collection intact  
    /// **Note:** will panic if called on an empty array
    fn merge(&self, add_new_line: bool) -> std::io::Error;
}
impl Merge for [std::io::Error] {
    fn merge(&self, add_new_line: bool) -> std::io::Error {
        if self.is_empty() {
            panic!("Tried to merge 0 errors");
        }
        let mut new_err: std::io::Error = self[0].clone_err();
        if self.len() > 1 {
            self.iter()
                .skip(1)
                .for_each(|err| new_err.add_msg(&err.to_string(), add_new_line));
        }
        new_err
    }
}

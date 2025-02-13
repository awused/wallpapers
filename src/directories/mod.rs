use std::cmp::max;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Error;
use std::path::Path;

use once_cell::sync::Lazy;
use regex::Regex;
use walkdir::{DirEntry, WalkDir};

use self::ids::OriginalWallpaperID;
use crate::config::CONFIG;

pub mod ids;

static FILE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^((.*\D)?)(\d+)\.[a-zA-Z]{3,4}$").unwrap());

static EXTENSIONS: [&str; 4] = ["jpg", "jpeg", "png", "bmp"];

pub fn valid_extension(ext: &OsStr) -> bool {
    // While there are only a few extensions this is faster than hashing.
    EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(OsStr::new(e)))
}

// Gets all the originals as forward slash separated relative paths
pub fn get_all_originals() -> Result<Vec<OriginalWallpaperID>, Error> {
    let walk = WalkDir::new(&CONFIG.originals_directory)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;

    Ok(walk
        .into_iter()
        .map(DirEntry::into_path)
        .filter(|p| p.is_file())
        .filter(|p| p.extension().is_some_and(valid_extension))
        .map(|p| {
            OriginalWallpaperID::from_rel_path(
                p.strip_prefix(&CONFIG.originals_directory)
                    .expect("File in originals directory did not have correct path prefix."),
            )
        })
        .collect())
}

// Returns (prefix, next_number, max_digits)
// Only returns if the directory is empty or contains all files matching the same prefix
pub fn next_original_in_dir(abs_dir: &Path) -> Option<(OsString, usize, usize)> {
    if !abs_dir.exists() {
        // Default to two digits
        // Corresponds to "install new_or_empty_dir/*" -> new_or_empty_dir/00.ext
        return Some(("".into(), 0, 2));
    }

    if !abs_dir.is_dir() {
        return None;
    }

    let mut files = fs::read_dir(abs_dir)
        .ok()?
        .flatten()
        .map(|de| de.path())
        .filter(|p| p.is_file())
        .filter(|p| p.extension().is_some_and(valid_extension));

    let mut num;
    let mut digits;
    let prefix;

    match files.next() {
        Some(f) => {
            let fname = f.file_name()?.to_string_lossy();
            let c = FILE_REGEX.captures(&fname)?;
            let number = c[3].parse::<usize>().ok()?;
            num = number;
            digits = c[3].len();
            prefix = c[1].to_string();
        }
        None => return Some(("".into(), 0, 2)),
    }

    for f in files {
        let fname = f.file_name()?.to_string_lossy();
        let c = FILE_REGEX.captures(&fname)?;
        if c[1] != prefix {
            return None;
        }

        let number = c[3].parse::<usize>().ok()?;
        num = max(num, number);
        digits = max(digits, c[3].len());
    }

    num += 1;

    Some((prefix.into(), num, digits))
}

// Returns (prefix, next_number, max_digits)
// Only returns values if the prefix identifies a single sequence of existing files.
pub fn next_original_for_prefix(abs_dir: &Path, prefix: &str) -> Option<(OsString, usize, usize)> {
    if !abs_dir.is_dir() {
        return None;
    }

    let mut files = fs::read_dir(abs_dir)
        .ok()?
        .flatten()
        .map(|de| de.path())
        .filter(|p| p.is_file())
        .filter(|p| p.extension().is_some_and(valid_extension))
        .filter(|p| p.file_name().is_some_and(|f| f.to_string_lossy().starts_with(prefix)));


    let first = files.next()?;
    let fname = first.file_name()?.to_string_lossy();
    let c = FILE_REGEX.captures(&fname)?;

    let mut num = c[3].parse::<usize>().ok()?;
    let mut digits = c[3].len();
    let prefix = &c[1];

    for f in files {
        let fname = f.file_name()?.to_string_lossy();
        let c = FILE_REGEX.captures(&fname)?;
        if &c[1] != prefix {
            return None;
        }

        let number = c[3].parse::<usize>().ok()?;
        num = max(num, number);
        digits = max(digits, c[3].len());
    }

    num += 1;

    Some((prefix.into(), num, digits))
}

// Returns (next_number, max_digits)
pub fn next_original_for_wildcard_prefix(abs_dir: &Path, prefix: &str) -> Option<(usize, usize)> {
    if !abs_dir.exists() {
        // Default to two digits
        // Corresponds to "install new_dir/prefix*" -> new_dir/prefix00.ext*
        return Some((0, 2));
    }

    if !abs_dir.is_dir() {
        return None;
    }

    let mut num = 0;
    let mut digits = 0;

    let files = fs::read_dir(abs_dir)
        .ok()?
        .flatten()
        .map(|de| de.path())
        .filter(|p| p.is_file())
        .filter(|p| p.extension().is_some_and(valid_extension));

    for f in files {
        let fname = f.file_name()?.to_string_lossy();
        let c = match FILE_REGEX.captures(&fname) {
            Some(c) if &c[1] == prefix => c,
            Some(_) | None => continue,
        };

        let number = c[3].parse::<usize>().ok()?;
        num = max(num, number + 1);
        digits = max(digits, c[3].len());
    }

    if digits == 0 {
        digits = 2;
    }

    Some((num, digits))
}

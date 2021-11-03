use std::collections::HashMap;
use std::fmt::Display;
use std::fs::{create_dir, read_to_string};
use std::path::PathBuf;
use std::string::ToString;

use image::Rgba;
use once_cell::sync::Lazy;
use serde::{Deserialize, Deserializer};

use crate::OPTIONS;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub database: PathBuf,

    pub originals_directory: PathBuf,

    pub cache_directory: PathBuf,

    #[serde(default, deserialize_with = "empty_path_is_none")]
    pub temp_dir: Option<PathBuf>,

    #[serde(default, deserialize_with = "empty_path_is_none")]
    pub alternate_upscaler: Option<PathBuf>,

    #[serde(default = "one")]
    pub upscaling_jobs: usize,
}

const fn one() -> usize {
    1
}

#[derive(Debug, Deserialize, Default)]
pub struct ImageProperties {
    pub vertical: Option<f64>,
    pub horizontal: Option<f64>,
    #[serde(default, deserialize_with = "zero_is_none")]
    pub top: Option<i32>,
    #[serde(default, deserialize_with = "zero_is_none")]
    pub bottom: Option<i32>,
    #[serde(default, deserialize_with = "zero_is_none")]
    pub left: Option<i32>,
    #[serde(default, deserialize_with = "zero_is_none")]
    pub right: Option<i32>,

    #[serde(default, deserialize_with = "deserialize_colour")]
    pub background: Option<Rgba<u8>>,

    #[serde(default, deserialize_with = "zero_is_none")]
    pub denoise: Option<i32>,

    // To facilitate deserializing
    #[serde(flatten)]
    pub nested: HashMap<String, HashMap<String, ImageProperties>>,
}

impl Display for ImageProperties {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        static FIELDS: [&str; 7] = [
            "vertical",
            "horizontal",
            "top",
            "bottom",
            "left",
            "right",
            "denoise",
        ];
        let values = [
            &self.vertical.as_ref().map(ToString::to_string),
            &self.horizontal.as_ref().map(ToString::to_string),
            &self.top.as_ref().map(ToString::to_string),
            &self.bottom.as_ref().map(ToString::to_string),
            &self.left.as_ref().map(ToString::to_string),
            &self.right.as_ref().map(ToString::to_string),
            &self.denoise.as_ref().map(ToString::to_string),
        ];
        FIELDS
            .iter()
            .zip(values.into_iter())
            .filter_map(|(a, b)| b.as_ref().map(|b| (a, b)))
            .map(|(a, b)| writeln!(f, "{} = {}", a, b))
            .collect::<Result<Vec<_>, _>>()?;

        if let Some(b) = self.background.as_ref() {
            writeln!(f, "background = {}", colour_to_string(*b))?;
        }

        Ok(())
    }
}

impl Clone for ImageProperties {
    fn clone(&self) -> Self {
        Self {
            vertical: self.vertical,
            horizontal: self.horizontal,
            top: self.top,
            bottom: self.bottom,
            left: self.left,
            right: self.right,
            background: self.background,
            denoise: self.denoise,
            nested: HashMap::new(),
        }
    }
}

impl ImageProperties {
    pub fn crop_pad_string(&self) -> String {
        if self.top.is_none()
            && self.bottom.is_none()
            && self.left.is_none()
            && self.right.is_none()
            && self.background.is_none()
        {
            return "".into();
        }

        [
            self.top.unwrap_or_default().to_string(),
            self.bottom.unwrap_or_default().to_string(),
            self.left.unwrap_or_default().to_string(),
            self.right.unwrap_or_default().to_string(),
            self.background.map_or_else(
                || "".to_string(),
                |v| v[0].to_string() + &v[1].to_string() + &v[2].to_string() + &v[3].to_string(),
            ),
        ]
        .join(",")
    }

    pub fn full_string(&self) -> String {
        let s = self.crop_pad_string();
        if self.vertical.is_none() && self.horizontal.is_none() && self.denoise.is_none() {
            return s;
        }

        s + "-"
            + &self.denoise.unwrap_or_default().to_string()
            + ","
            + &self.vertical.unwrap_or_default().to_string()
            + ","
            + &self.horizontal.unwrap_or_default().to_string()
    }
}

#[derive(Debug, Deserialize)]
pub struct Properties {}


// Serde seems broken with OsString for some reason
fn empty_path_is_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: From<PathBuf>,
{
    let s = PathBuf::deserialize(deserializer)?;
    if s.as_os_str().is_empty() {
        Ok(None)
    } else {
        Ok(Some(s.into()))
    }
}

fn zero_is_none<'de, D>(deserializer: D) -> Result<Option<i32>, D::Error>
where
    D: Deserializer<'de>,
{
    let i = i32::deserialize(deserializer)?;
    if i != 0 { Ok(Some(i)) } else { Ok(None) }
}

fn deserialize_colour<'de, D>(deserializer: D) -> Result<Option<Rgba<u8>>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;

    Ok(Some(
        string_to_colour(&s).unwrap_or_else(|| panic!("Unable to parse colour {}", s)),
    ))
}

pub fn string_to_colour(s: &str) -> Option<Rgba<u8>> {
    let s = s.to_ascii_lowercase();
    // Only hardcode these since they're very common.
    if s == "black" {
        return Some([0, 0, 0, 0xff].into());
    } else if s == "white" {
        return Some([0xff, 0xff, 0xff, 0xff].into());
    }

    if s.len() != 6 {
        return None;
    }
    let c = u32::from_str_radix(&s, 16).ok()?;

    Some(
        [
            (c >> 16) as u8,
            ((c >> 8) & 0xff) as u8,
            (c & 0xff) as u8,
            0xff,
        ]
        .into(),
    )
}

fn colour_to_string(c: Rgba<u8>) -> String {
    let (r, g, b) = (c[0], c[1], c[2]);

    match (r, g, b) {
        (0, 0, 0) => "black".into(),
        (0xff, 0xff, 0xff) => "white".into(),
        _ => format!("{:02x}{:02x}{:02x}", r, g, b),
    }
}

pub static CONFIG: Lazy<Config> = Lazy::new(|| {
    let config =
        awconf::load_config::<Config>("wallpapers", &OPTIONS.awconf).expect("Error loading config");
    assert!(
        config.originals_directory.is_dir(),
        "Originals directory {:?} is not a directory",
        config.originals_directory
    );

    if !config.cache_directory.exists() {
        create_dir(&config.cache_directory).unwrap();
    }

    assert!(
        config.cache_directory.is_dir(),
        "Cache directory {:?} is not a directory",
        config.cache_directory
    );

    assert!(config.upscaling_jobs > 0, "Upscaling jobs cannot be 0");

    config
});


pub static PROPERTIES: Lazy<HashMap<PathBuf, ImageProperties>> = Lazy::new(|| {
    let propfile = CONFIG.originals_directory.join(".properties.toml");
    if !propfile.is_file() {
        return HashMap::new();
    }

    // TOML files are UTF-8 by definition
    let properties = read_to_string(&propfile).expect("Error reading properties file");

    let mut deserializer = toml::Deserializer::new(&properties);

    HashMap::<PathBuf, ImageProperties>::deserialize(&mut deserializer)
        .expect("Unable to deserialize properties")
});

use std::collections::BTreeMap;
use std::fmt::Display;
use std::fs::{create_dir, read_to_string};
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::string::ToString;

use image::Rgba;
use once_cell::sync::Lazy;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

    #[cfg_attr(any(not(feature = "opencl"), test), allow(unused))]
    #[serde(default)]
    pub gpu_prefix: String,
}

const fn one() -> usize {
    1
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ImageProperties {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertical: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub horizontal: Option<f64>,
    #[serde(default, deserialize_with = "zero_is_none")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<i32>,
    #[serde(default, deserialize_with = "zero_is_none")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom: Option<i32>,
    #[serde(default, deserialize_with = "zero_is_none")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left: Option<i32>,
    #[serde(default, deserialize_with = "zero_is_none")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right: Option<i32>,

    #[serde(
        default,
        deserialize_with = "deserialize_colour",
        serialize_with = "serialize_colour"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<Rgba<u8>>,

    #[serde(default, deserialize_with = "zero_is_none")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub denoise: Option<i32>,

    // To facilitate deserializing
    #[serde(flatten)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(serialize_with = "skip_nested_empties")]
    pub nested: BTreeMap<String, BTreeMap<String, ImageProperties>>,
}

impl Display for ImageProperties {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        static FIELDS: [&str; 7] =
            ["vertical", "horizontal", "top", "bottom", "left", "right", "denoise"];
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
            .map(|(a, b)| writeln!(f, "{a} = {b}"))
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
            nested: BTreeMap::new(),
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

    pub fn is_empty(&self) -> bool {
        for n in self.nested.values() {
            for n in n.values() {
                if !n.is_empty() {
                    return false;
                }
            }
        }

        self.full_string().is_empty()
    }

    pub fn copy_from(&mut self, other: &Self) {
        self.vertical = other.vertical;
        self.horizontal = other.horizontal;
        self.top = other.top;
        self.bottom = other.bottom;
        self.left = other.left;
        self.right = other.right;
        self.background = other.background;
        self.denoise = other.denoise;
    }
}

// Serde seems broken with OsString for some reason
fn empty_path_is_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: From<PathBuf>,
{
    let s = PathBuf::deserialize(deserializer)?;
    if s.as_os_str().is_empty() { Ok(None) } else { Ok(Some(s.into())) }
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
        string_to_colour(&s).unwrap_or_else(|| panic!("Unable to parse colour {s}")),
    ))
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn serialize_colour<S>(v: &Option<Rgba<u8>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    // We skip serializing if the option is none anyway
    serializer.serialize_str(&colour_to_string(*v.as_ref().unwrap()))
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

    Some([(c >> 16) as u8, ((c >> 8) & 0xff) as u8, (c & 0xff) as u8, 0xff].into())
}

fn colour_to_string(c: Rgba<u8>) -> String {
    let (r, g, b) = (c[0], c[1], c[2]);

    match (r, g, b) {
        (0, 0, 0) => "black".into(),
        (0xff, 0xff, 0xff) => "white".into(),
        _ => format!("{r:02x}{g:02x}{b:02x}"),
    }
}

pub static CONFIG: Lazy<Config> = Lazy::new(|| {
    let (config, _) =
        awconf::load_config::<Config>("wallpapers", OPTIONS.awconf.as_ref(), None::<&str>)
            .expect("Error loading config");
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


#[derive(Serialize, Deserialize)]
pub struct Properties {
    #[serde(flatten)]
    #[serde(serialize_with = "skip_empties")]
    props: BTreeMap<PathBuf, ImageProperties>,
}

impl Deref for Properties {
    type Target = BTreeMap<PathBuf, ImageProperties>;

    fn deref(&self) -> &Self::Target {
        &self.props
    }
}

impl DerefMut for Properties {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.props
    }
}

fn skip_empties<T, S>(map: &BTreeMap<T, ImageProperties>, serializer: S) -> Result<S::Ok, S::Error>
where
    T: Serialize,
    S: Serializer,
{
    let filtered: Vec<_> = map.iter().filter(|(_, v)| !v.is_empty()).collect();
    let mut map_ser = serializer.serialize_map(Some(filtered.len()))?;

    for (k, v) in filtered {
        map_ser.serialize_entry(k, v)?;
    }
    map_ser.end()
}

fn skip_nested_empties<S>(
    map: &BTreeMap<String, BTreeMap<String, ImageProperties>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let filtered: Vec<_> = map
        .iter()
        .filter(|(_, v)| {
            for ip in v.values() {
                if !ip.is_empty() {
                    return true;
                }
            }
            false
        })
        .collect();
    let mut map_ser = serializer.serialize_map(Some(filtered.len()))?;

    // Empties can still get through if there's a sibling non-empty, but this is good enough for
    // now.
    for (k, v) in filtered {
        map_ser.serialize_entry(k, v)?;
    }
    map_ser.end()
}


pub fn load_properties() -> Properties {
    let propfile = CONFIG.originals_directory.join(".properties.toml");
    if !propfile.is_file() {
        return Properties { props: BTreeMap::new() };
    }

    // TOML files are UTF-8 by definition
    let properties = read_to_string(&propfile).expect("Error reading properties file");

    let deserializer = toml::Deserializer::parse(&properties).expect("Unable to parse properties");

    Properties::deserialize(deserializer).expect("Unable to deserialize properties")
}

pub static PROPERTIES: Lazy<Properties> = Lazy::new(load_properties);

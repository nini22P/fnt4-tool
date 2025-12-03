use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::glyph::LazyGlyph;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FntMetadata {
    pub version: FntVersion,
    pub mipmap_level: usize,
    pub ascent: u16,
    pub descent: u16,
    #[serde(with = "hex_character")]
    pub characters: BTreeMap<u32, u32>, // Maps character code to glyph ID
    pub glyphs: BTreeMap<u32, GlyphMetadata>, // glyph_id -> glyph_metadata
}

impl FntMetadata {
    pub fn read_metadata(path: &Path) -> io::Result<FntMetadata> {
        let file = std::fs::File::open(path)?;
        let mut reader = io::BufReader::new(file);

        let mut content = String::new();
        reader.read_to_string(&mut content)?;

        let metadata: FntMetadata = toml::from_str(&content).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("TOML parsing error: {}", e),
            )
        })?;

        Ok(metadata)
    }

    pub fn write_metadata(&self, path: &Path) -> io::Result<()> {
        let content = toml::to_string_pretty(self).map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("TOML serialization error: {}", e),
            )
        })?;

        let file = std::fs::File::create(path)?;

        io::BufWriter::new(file).write_all(content.as_bytes())?;

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FntVersion {
    V0 = 0,
    V1 = 1,
}

impl FntVersion {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(FntVersion::V0),
            1 => Some(FntVersion::V1),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodeType {
    Unicode,
    Sjis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlyphMetadata {
    #[serde(with = "hex_string")]
    pub char_code: u32,
    pub code_type: CodeType,
    pub bearing_x: i8,
    pub bearing_y: i8,
    pub advance: u8,
}

pub fn detect_mipmap_level(lazy_glyphs: &BTreeMap<u32, LazyGlyph>) -> usize {
    let mut max_levels = 1usize;

    for lazy_glyph in lazy_glyphs.values().take(10) {
        let (tw, th) = lazy_glyph.texture_size;
        if tw == 0 || th == 0 {
            continue;
        }

        let decompressed = lazy_glyph.glyph_data.decompress(10, 2);
        let mut pos = 0;
        let mut levels = 0;

        for level in 0..4 {
            let w = (tw as usize) >> level;
            let h = (th as usize) >> level;
            if w == 0 || h == 0 {
                break;
            }

            let expected_size = w * h;
            if pos + expected_size > decompressed.len() {
                break;
            }

            pos += expected_size;
            levels = level + 1;
        }

        if levels > max_levels {
            max_levels = levels;
        }
    }

    max_levels
}

mod hex_character {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(map: &BTreeMap<u32, u32>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hex_map: BTreeMap<String, u32> = map
            .iter()
            .map(|(&k, &v)| {
                let hex_key = format!("{:04X}", k);
                (hex_key, v)
            })
            .collect();
        hex_map.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<BTreeMap<u32, u32>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let hex_map: BTreeMap<String, u32> = BTreeMap::deserialize(deserializer)?;

        let mut map = BTreeMap::new();

        for (hex_key, v) in hex_map {
            let key_result = if hex_key.starts_with("0x") {
                u32::from_str_radix(&hex_key[2..], 16)
            } else {
                u32::from_str_radix(&hex_key, 16)
            };

            let k = key_result.map_err(serde::de::Error::custom)?;
            map.insert(k, v);
        }

        Ok(map)
    }
}

mod hex_string {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &u32, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = format!("{:04X}", value);
        serializer.serialize_str(&s)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u32, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s.starts_with("0x") {
            u32::from_str_radix(&s[2..], 16).map_err(serde::de::Error::custom)
        } else {
            u32::from_str_radix(&s, 16).map_err(serde::de::Error::custom)
        }
    }
}

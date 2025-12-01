use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphMipLevel {
    Level0 = 0,
    Level1 = 1,
    Level2 = 2,
    Level3 = 3,
}

#[derive(Debug, Clone)]
pub struct FntHeader {
    pub magic: [u8; 4],
    pub version: FntVersion,
    pub file_size: u32,
    pub ascent: u16,
    pub descent: u16,
}

impl FntHeader {
    pub const SIZE_V0: usize = 16;
    pub const SIZE_V1: usize = 16;

    pub fn parse(data: &[u8]) -> Result<Self, &'static str> {
        if data.len() < 16 {
            return Err("Data too short for FNT4 header");
        }

        let magic: [u8; 4] = data[0..4].try_into().unwrap();
        if &magic != b"FNT4" {
            return Err("Invalid magic number");
        }

        // Check version based on data layout
        if data[0x4..0x8] == [0x01, 0x00, 0x00, 0x00] {
            // Version 1
            let version = FntVersion::V1;
            let file_size = u32::from_le_bytes(data[8..12].try_into().unwrap());
            let ascent = u16::from_le_bytes(data[12..14].try_into().unwrap());
            let descent = u16::from_le_bytes(data[14..16].try_into().unwrap());

            Ok(FntHeader {
                magic,
                version,
                file_size,
                ascent,
                descent,
            })
        } else if data[0xC..0x10] == [0x00, 0x00, 0x00, 0x00] {
            // Version 0
            let version = FntVersion::V0;
            let file_size = u32::from_le_bytes(data[4..8].try_into().unwrap());
            let ascent = u16::from_le_bytes(data[8..10].try_into().unwrap());
            let descent = u16::from_le_bytes(data[10..12].try_into().unwrap());

            Ok(FntHeader {
                magic,
                version,
                file_size,
                ascent,
                descent,
            })
        } else {
            Err("Unknown FNT4 version")
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut result = Vec::with_capacity(16);
        result.extend_from_slice(&self.magic);

        match self.version {
            FntVersion::V1 => {
                result.extend_from_slice(&1u32.to_le_bytes());
                result.extend_from_slice(&self.file_size.to_le_bytes());
                result.extend_from_slice(&self.ascent.to_le_bytes());
                result.extend_from_slice(&self.descent.to_le_bytes());
            }
            FntVersion::V0 => {
                result.extend_from_slice(&self.file_size.to_le_bytes());
                result.extend_from_slice(&self.ascent.to_le_bytes());
                result.extend_from_slice(&self.descent.to_le_bytes());
                result.extend_from_slice(&0u32.to_le_bytes());
            }
        }
        result
    }

    pub fn size(&self) -> usize {
        match self.version {
            FntVersion::V0 => Self::SIZE_V0,
            FntVersion::V1 => Self::SIZE_V1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GlyphHeader {
    pub bearing_x: i8,
    pub bearing_y: i8,
    pub actual_width: u8,
    pub actual_height: u8,
    pub advance_width: u8,
    pub unused: u8,         // Always 0 for v1, unknown for v0
    pub texture_width: u8,  // Only for v1
    pub texture_height: u8, // Only for v1
    pub compressed_size: u16,
}

impl GlyphHeader {
    pub const SIZE_V0: usize = 8;
    pub const SIZE_V1: usize = 10;

    pub fn parse(data: &[u8], offset: usize, version: FntVersion) -> Result<Self, &'static str> {
        match version {
            FntVersion::V1 => {
                if data.len() < offset + Self::SIZE_V1 {
                    return Err("Data too short for glyph header v1");
                }
                Ok(GlyphHeader {
                    bearing_x: data[offset] as i8,
                    bearing_y: data[offset + 1] as i8,
                    actual_width: data[offset + 2],
                    actual_height: data[offset + 3],
                    advance_width: data[offset + 4],
                    unused: data[offset + 5],
                    texture_width: data[offset + 6],
                    texture_height: data[offset + 7],
                    compressed_size: u16::from_le_bytes(
                        data[offset + 8..offset + 10].try_into().unwrap(),
                    ),
                })
            }
            FntVersion::V0 => {
                if data.len() < offset + Self::SIZE_V0 {
                    return Err("Data too short for glyph header v0");
                }
                Ok(GlyphHeader {
                    bearing_x: data[offset] as i8,
                    bearing_y: data[offset + 1] as i8,
                    actual_width: data[offset + 2],
                    actual_height: data[offset + 3],
                    advance_width: data[offset + 4],
                    unused: data[offset + 5],
                    texture_width: 0, // Not used in v0
                    texture_height: 0,
                    compressed_size: u16::from_le_bytes(
                        data[offset + 6..offset + 8].try_into().unwrap(),
                    ),
                })
            }
        }
    }

    pub fn size(&self, version: FntVersion) -> usize {
        match version {
            FntVersion::V0 => Self::SIZE_V0,
            FntVersion::V1 => Self::SIZE_V1,
        }
    }

    pub fn to_bytes_v1(&self) -> Vec<u8> {
        let mut result = Vec::with_capacity(Self::SIZE_V1);
        result.push(self.bearing_x as u8);
        result.push(self.bearing_y as u8);
        result.push(self.actual_width);
        result.push(self.actual_height);
        result.push(self.advance_width);
        result.push(self.unused);
        result.push(self.texture_width);
        result.push(self.texture_height);
        result.extend_from_slice(&self.compressed_size.to_le_bytes());
        result
    }
}

#[derive(Debug, Clone)]
pub struct GlyphInfo {
    pub bearing_x: i8,
    pub bearing_y: i8,
    pub advance_width: u8,
    pub actual_width: u8,
    pub actual_height: u8,
    pub texture_width: u8,
    pub texture_height: u8,
    pub char_code: u32, // Unicode for v1, SJIS codepoint for v0
}

impl GlyphInfo {
    pub fn from_header(header: &GlyphHeader, char_code: u32, version: FntVersion) -> Self {
        GlyphInfo {
            bearing_x: header.bearing_x,
            bearing_y: header.bearing_y,
            advance_width: header.advance_width,
            actual_width: header.actual_width,
            actual_height: header.actual_height,
            texture_width: if version == FntVersion::V1 {
                header.texture_width
            } else {
                header.actual_width
            },
            texture_height: if version == FntVersion::V1 {
                header.texture_height
            } else {
                header.actual_height
            },
            char_code,
        }
    }

    pub fn actual_size(&self) -> (u8, u8) {
        (self.actual_width, self.actual_height)
    }

    pub fn texture_size(&self) -> (u8, u8) {
        (self.texture_width, self.texture_height)
    }
}

#[derive(Debug, Clone)]
pub struct GlyphData {
    pub data: Vec<u8>,
    pub is_compressed: bool,
}

impl GlyphData {
    pub fn decompress(&self, seek_bits: usize, backseek_nbyte: usize) -> Vec<u8> {
        if self.is_compressed {
            crate::lz77::decompress(&self.data, seek_bits, backseek_nbyte)
        } else {
            self.data.clone()
        }
    }
}

#[derive(Debug, Clone)]
pub struct LazyGlyph {
    pub info: GlyphInfo,
    pub texture_size: (u8, u8),
    pub glyph_data: GlyphData,
}

#[derive(Debug)]
pub struct Glyph {
    pub info: GlyphInfo,
    pub mip_level_0: Vec<u8>,
    pub mip_level_1: Option<Vec<u8>>,
    pub mip_level_2: Option<Vec<u8>>,
    pub mip_level_3: Option<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug)]
pub struct Fnt {
    pub version: FntVersion,
    pub ascent: u16,
    pub descent: u16,
    pub character_table_crc: u32,
    pub characters: Vec<u32>,             // Maps character code to glyph ID
    pub glyphs: BTreeMap<u32, LazyGlyph>, // Glyph ID to LazyGlyph
    pub glyph_offsets: Vec<u32>,          // Debug: character table offsets
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FntMetadata {
    pub version: FntVersion,
    pub mipmap_levels: usize,
    pub ascent: u16,
    pub descent: u16,
    pub glyphs: BTreeMap<u32, GlyphMetadata>, // glyph_id -> glyph_metadata
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

mod hex_string {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &u32, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = format!("0x{:04X}", value);
        serializer.serialize_str(&s)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u32, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let s = s.trim_start_matches("0x");
        u32::from_str_radix(s, 16).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug)]
pub struct ProcessedGlyph {
    pub glyph_info: GlyphMetadata,
    pub actual_width: u8,
    pub actual_height: u8,
    pub texture_width: u8,
    pub texture_height: u8,
    pub data_to_write: Vec<u8>,
    pub compressed_size: u16,
}

pub struct RenderedGlyph {
    pub bearing_x: i8,
    pub bearing_y: i8,
    pub advance_width: u8,
    pub actual_width: u8,
    pub actual_height: u8,
    pub alpha_data: Vec<u8>,
}

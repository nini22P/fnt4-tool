use std::collections::BTreeMap;

use crate::{
    crc32,
    glyph::{GlyphData, GlyphHeader, GlyphInfo, LazyGlyph, ProcessedGlyph},
    metadata::{CodeType, FntMetadata, FntVersion, GlyphMetadata, detect_mipmap_level},
    utils::generate_sjis_map,
};

#[derive(Debug)]
pub struct Fnt {
    pub metadata: FntMetadata,
    pub lazy_glyphs: BTreeMap<u32, LazyGlyph>,
    pub glyph_offsets: Vec<u32>,
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

impl Fnt {
    pub fn from_data(data: &[u8]) -> Result<Fnt, &'static str> {
        let header = FntHeader::parse(data)?;

        if header.file_size as usize != data.len() {
            return Err("FNT4 font size in header does not match actual data size");
        }

        // Calculate character table size
        let first_glyph_offset = u32::from_le_bytes(data[0x10..0x14].try_into().unwrap());
        let character_size = ((first_glyph_offset as usize) - 0x10) / 4;

        // Read character table
        let mut character_table: Vec<u32> = Vec::with_capacity(character_size);
        for i in 0..character_size {
            let start = i * 4 + header.size();
            let offset = u32::from_le_bytes(data[start..start + 4].try_into().unwrap());
            character_table.push(offset);
        }

        // Calculate character table CRC32
        let mut character_table_bytes = Vec::with_capacity(character_table.len() * 4);
        for offset in &character_table {
            character_table_bytes.extend_from_slice(&offset.to_le_bytes());
        }
        let character_table_crc = crc32::crc32(&character_table_bytes, 0);

        let sjis_map = if header.version == FntVersion::V0 {
            Some(generate_sjis_map())
        } else {
            None
        };

        // Read glyph data
        let mut known_glyph_offsets: BTreeMap<u32, u32> = BTreeMap::new();
        let mut characters: BTreeMap<u32, u32> = BTreeMap::new();
        let mut lazy_glyphs: BTreeMap<u32, LazyGlyph> = BTreeMap::new();

        for (character_index, &glyph_offset) in character_table.iter().enumerate() {
            let glyph_id = if let Some(&id) = known_glyph_offsets.get(&glyph_offset) {
                id
            } else {
                let id = known_glyph_offsets.len() as u32;
                known_glyph_offsets.insert(glyph_offset, id);
                id
            };

            characters.insert(character_index as u32, glyph_id);

            if lazy_glyphs.contains_key(&glyph_id) {
                continue;
            }

            let char_code = if let Some(map) = &sjis_map {
                *map.get(character_index).unwrap_or(&0)
            } else {
                character_index as u32
            };

            let lazy_glyph =
                LazyGlyph::from_data(data, glyph_offset as usize, char_code, header.version)?;
            lazy_glyphs.insert(glyph_id, lazy_glyph);
        }

        let mut glyphs = BTreeMap::new();

        let code_type = if header.version == FntVersion::V0 {
            CodeType::Sjis
        } else {
            CodeType::Unicode
        };

        let mipmap_level = detect_mipmap_level(&lazy_glyphs);

        for (glyph_id, lazy_glyph) in lazy_glyphs.clone() {
            let info = &lazy_glyph.info;
            glyphs.insert(
                glyph_id,
                GlyphMetadata {
                    char_code: info.char_code,
                    code_type: code_type.clone(),
                    bearing_x: info.bearing_x,
                    bearing_y: info.bearing_y,
                    advance: info.advance,
                },
            );
        }

        let metadata = FntMetadata {
            version: header.version,
            mipmap_level,
            ascent: header.ascent,
            descent: header.descent,
            character_table_crc,
            characters,
            glyphs,
        };

        Ok(Fnt {
            metadata,
            lazy_glyphs,
            glyph_offsets: character_table,
        })
    }
}

impl Fnt {
    pub fn from_processed_glyphs(
        metadata: FntMetadata,
        processed_glyphs: BTreeMap<u32, ProcessedGlyph>,
    ) -> Self {
        let mut lazy_glyphs = BTreeMap::new();

        for (id, pg) in processed_glyphs {
            let is_compressed = pg.compressed_size > 0;

            let glyph_data = GlyphData {
                data: pg.data,
                is_compressed,
            };

            let info = GlyphInfo {
                bearing_x: pg.glyph_info.bearing_x,
                bearing_y: pg.glyph_info.bearing_y,
                advance: pg.glyph_info.advance,
                actual_width: pg.actual_width,
                actual_height: pg.actual_height,
                texture_width: pg.texture_width,
                texture_height: pg.texture_height,
                char_code: pg.glyph_info.char_code,
            };

            lazy_glyphs.insert(
                id,
                LazyGlyph {
                    info,
                    texture_size: (pg.texture_width, pg.texture_height),
                    glyph_data,
                },
            );
        }

        let mut characters = vec![0u32; 65536];

        let default_glyph_id = lazy_glyphs.keys().min().copied().unwrap_or(0);

        for c in characters.iter_mut() {
            *c = default_glyph_id;
        }

        for (glyph_id, glyph) in metadata.clone().glyphs {
            characters[glyph.char_code as usize] = glyph_id;
        }

        Fnt {
            metadata,
            lazy_glyphs,
            glyph_offsets: Vec::new(),
        }
    }
}

impl Fnt {
    fn write<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let header_size = 16usize;
        let character_table_size = 65536 * 4;

        let mut lazy_glyphs = self.lazy_glyphs.clone();

        let mut current_offset = header_size + character_table_size;
        let mut glyph_id_to_offset: BTreeMap<u32, u32> = BTreeMap::new();

        for (glyph_id, lazy_glyph) in &lazy_glyphs {
            glyph_id_to_offset.insert(*glyph_id, current_offset as u32);

            let header_len = match self.metadata.version {
                FntVersion::V0 => GlyphHeader::SIZE_V0,
                FntVersion::V1 => GlyphHeader::SIZE_V1,
            };
            let data_len = lazy_glyph.glyph_data.data.len();

            current_offset += header_len + data_len;
        }

        let total_file_size = current_offset as u32;

        let header = FntHeader {
            magic: *b"FNT4",
            version: self.metadata.version,
            file_size: total_file_size,
            ascent: self.metadata.ascent,
            descent: self.metadata.descent,
        };
        writer.write_all(&header.to_bytes())?;

        for (character_index, glyph_id) in &self.metadata.characters {
            let offset = *glyph_id_to_offset.get(&glyph_id).unwrap_or(&0);

            let final_offset = if offset == 0 {
                *glyph_id_to_offset
                    .get(lazy_glyphs.first_entry().unwrap().key())
                    .unwrap_or(&(header_size as u32 + character_table_size as u32))
            } else {
                offset
            };

            writer.write_all(&final_offset.to_le_bytes())?;
        }

        for (glyph_id, lazy_glyph) in lazy_glyphs {
            let compressed_size = if lazy_glyph.glyph_data.is_compressed {
                lazy_glyph.glyph_data.data.len() as u16
            } else {
                0
            };

            let glyph_header = GlyphHeader {
                bearing_x: lazy_glyph.info.bearing_x,
                bearing_y: lazy_glyph.info.bearing_y,
                actual_width: lazy_glyph.info.actual_width,
                actual_height: lazy_glyph.info.actual_height,
                advance: lazy_glyph.info.advance,
                unused: 0,
                texture_width: lazy_glyph.info.texture_width,
                texture_height: lazy_glyph.info.texture_height,
                compressed_size,
            };

            match self.metadata.version {
                FntVersion::V1 => writer.write_all(&glyph_header.to_bytes_v1())?,
                FntVersion::V0 => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "FNT4 V0 not supported",
                    ));
                }
            }

            writer.write_all(&lazy_glyph.glyph_data.data)?;
        }

        Ok(())
    }
}

impl Fnt {
    pub fn read_fnt(path: &std::path::Path) -> Result<Fnt, &'static str> {
        let data = std::fs::read(path).map_err(|_| "Failed to read FNT4 font file")?;
        Self::from_data(&data)
    }
}

impl Fnt {
    pub fn write_fnt(&self, path: &std::path::Path) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;
        self.write(&mut file)
    }
}

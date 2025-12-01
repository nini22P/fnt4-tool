use std::collections::BTreeMap;

use crate::types::{
    Fnt, FntHeader, FntMetadata, FntVersion, GlyphData, GlyphHeader, GlyphInfo, LazyGlyph,
    ProcessedGlyph,
};

impl Fnt {
    pub fn from_processed_data(
        metadata: FntMetadata,
        processed_glyphs: BTreeMap<u32, ProcessedGlyph>,
        version: FntVersion,
    ) -> Self {
        let mut glyphs = BTreeMap::new();

        for (id, pg) in processed_glyphs {
            let is_compressed = pg.compressed_size > 0;

            let glyph_data = GlyphData {
                data: pg.data_to_write,
                is_compressed,
            };

            let info = GlyphInfo {
                bearing_x: pg.glyph_info.bearing_x,
                bearing_y: pg.glyph_info.bearing_y,
                advance_width: pg.glyph_info.advance,
                actual_width: pg.actual_width,
                actual_height: pg.actual_height,
                texture_width: pg.texture_width,
                texture_height: pg.texture_height,
                char_code: pg.glyph_info.char_code,
            };

            glyphs.insert(
                id,
                LazyGlyph {
                    info,
                    texture_size: (pg.texture_width, pg.texture_height),
                    glyph_data,
                },
            );
        }

        let mut characters = vec![0u32; 65536];

        let default_glyph_id = glyphs.keys().min().copied().unwrap_or(0);

        for c in characters.iter_mut() {
            *c = default_glyph_id;
        }

        for (glyph_id, glyph) in metadata.glyphs {
            characters[glyph.char_code as usize] = glyph_id;
        }

        Fnt {
            version,
            ascent: metadata.ascent,
            descent: metadata.descent,
            character_table_crc: 0,
            characters,
            glyphs,
            glyph_offsets: Vec::new(),
        }
    }
}

impl Fnt {
    pub fn save_fnt(&self, path: &std::path::Path) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;
        self.write(&mut file)
    }
}

impl Fnt {
    fn write<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let header_size = 16usize;
        let character_table_size = 65536 * 4;

        let mut sorted_glyph_ids: Vec<u32> = self.glyphs.keys().copied().collect();
        sorted_glyph_ids.sort();

        let mut current_offset = header_size + character_table_size;
        let mut glyph_id_to_offset: BTreeMap<u32, u32> = BTreeMap::new();

        for &id in &sorted_glyph_ids {
            if let Some(glyph) = self.glyphs.get(&id) {
                glyph_id_to_offset.insert(id, current_offset as u32);

                let header_len = match self.version {
                    FntVersion::V0 => GlyphHeader::SIZE_V0,
                    FntVersion::V1 => GlyphHeader::SIZE_V1,
                };
                let data_len = glyph.glyph_data.data.len();

                current_offset += header_len + data_len;
            }
        }

        let total_file_size = current_offset as u32;

        let header = FntHeader {
            magic: *b"FNT4",
            version: self.version,
            file_size: total_file_size,
            ascent: self.ascent,
            descent: self.descent,
        };
        writer.write_all(&header.to_bytes())?;

        for glyph_id in &self.characters {
            let offset = *glyph_id_to_offset.get(glyph_id).unwrap_or(&0);

            let final_offset = if offset == 0 {
                *glyph_id_to_offset
                    .get(&sorted_glyph_ids[0])
                    .unwrap_or(&(header_size as u32 + character_table_size as u32))
            } else {
                offset
            };

            writer.write_all(&final_offset.to_le_bytes())?;
        }

        for &id in &sorted_glyph_ids {
            let glyph = self.glyphs.get(&id).unwrap();

            let compressed_size = if glyph.glyph_data.is_compressed {
                glyph.glyph_data.data.len() as u16
            } else {
                0
            };

            let glyph_header = GlyphHeader {
                bearing_x: glyph.info.bearing_x,
                bearing_y: glyph.info.bearing_y,
                actual_width: glyph.info.actual_width,
                actual_height: glyph.info.actual_height,
                advance_width: glyph.info.advance_width,
                unused: 0,
                texture_width: glyph.info.texture_width,
                texture_height: glyph.info.texture_height,
                compressed_size,
            };

            match self.version {
                FntVersion::V1 => writer.write_all(&glyph_header.to_bytes_v1())?,
                FntVersion::V0 => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "FNT4 V0 not supported",
                    ));
                }
            }

            writer.write_all(&glyph.glyph_data.data)?;
        }

        Ok(())
    }
}

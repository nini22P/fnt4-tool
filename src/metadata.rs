use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
    path::Path,
};

use crate::types::{CodeType, Fnt, FntMetadata, FntVersion, GlyphMetadata};

impl FntMetadata {
    pub fn parse_metadata(path: &Path) -> io::Result<FntMetadata> {
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

    pub fn save_metadata(&self, path: &Path) -> io::Result<()> {
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

impl Fnt {
    pub fn extract_metadata(&self) -> FntMetadata {
        let code_type = if self.version == FntVersion::V0 {
            CodeType::Sjis
        } else {
            CodeType::Unicode
        };
        let mipmap_levels = self.detect_mipmap_levels();

        let mut glyphs = BTreeMap::new();
        for (&glyph_id, lazy_glyph) in &self.glyphs {
            let info = &lazy_glyph.info;
            glyphs.insert(
                glyph_id,
                GlyphMetadata {
                    char_code: info.char_code,
                    code_type: code_type.clone(),
                    bearing_x: info.bearing_x,
                    bearing_y: info.bearing_y,
                    advance_width: info.advance_width,
                },
            );
        }

        FntMetadata {
            version: self.version,
            mipmap_levels,
            ascent: self.ascent,
            descent: self.descent,
            glyphs,
        }
    }
}

impl Fnt {
    fn detect_mipmap_levels(&self) -> usize {
        if self.version == FntVersion::V0 {
            return 1;
        }

        let mut max_levels = 1usize;

        for lazy_glyph in self.glyphs.values().take(10) {
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
}

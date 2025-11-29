use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use image::{Rgba, RgbaImage};
use rayon::prelude::*;

use super::types::*;
use crate::crc32;

pub fn read_font(data: &[u8]) -> Result<Font, &'static str> {
    let header = FontHeader::parse(data)?;

    if header.file_size as usize != data.len() {
        return Err("Font size in header does not match actual data size");
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

    // Read glyph data
    let mut known_glyph_offsets: HashMap<u32, u32> = HashMap::new();
    let mut characters: Vec<u32> = vec![0; character_size];
    let mut glyphs: HashMap<u32, LazyGlyph> = HashMap::new();

    for (character_index, &glyph_offset) in character_table.iter().enumerate() {
        let glyph_id = if let Some(&id) = known_glyph_offsets.get(&glyph_offset) {
            id
        } else {
            let id = known_glyph_offsets.len() as u32;
            known_glyph_offsets.insert(glyph_offset, id);
            id
        };

        characters[character_index] = glyph_id;

        if glyphs.contains_key(&glyph_id) {
            continue;
        }

        let lazy_glyph = read_lazy_glyph(
            data,
            glyph_offset as usize,
            character_index as u32,
            header.version,
        )?;
        glyphs.insert(glyph_id, lazy_glyph);
    }

    Ok(Font {
        version: header.version,
        ascent: header.ascent,
        descent: header.descent,
        character_table_crc,
        characters,
        glyphs,
        glyph_offsets: character_table,
    })
}

fn read_lazy_glyph(
    data: &[u8],
    offset: usize,
    char_code: u32,
    version: FontVersion,
) -> Result<LazyGlyph, &'static str> {
    let glyph_header = GlyphHeader::parse(data, offset, version)?;
    let compressed_size = glyph_header.compressed_size;

    let (texture_size, uncompressed_size) = match version {
        FontVersion::V1 => {
            let w = glyph_header.texture_width as usize;
            let h = glyph_header.texture_height as usize;
            let initial_mip_size = w * h;
            let total = initial_mip_size
                + (initial_mip_size / 4)
                + (initial_mip_size / 16)
                + (initial_mip_size / 64);
            (
                (glyph_header.texture_width, glyph_header.texture_height),
                total,
            )
        }
        FontVersion::V0 => {
            let w = glyph_header.actual_width as usize;
            let h = glyph_header.actual_height as usize;
            let stride = (w + 1) / 2; // ceil(width/2) for 4bpp
            (
                (glyph_header.actual_width, glyph_header.actual_height),
                stride * h,
            )
        }
    };

    let info = GlyphInfo::from_header(&glyph_header, char_code, version);
    let header_size = glyph_header.size(version);
    let data_start = offset + header_size;

    let (glyph_bytes, is_compressed) = if compressed_size == 0 {
        (
            data[data_start..data_start + uncompressed_size].to_vec(),
            false,
        )
    } else {
        (
            data[data_start..data_start + compressed_size as usize].to_vec(),
            true,
        )
    };

    Ok(LazyGlyph {
        info,
        texture_size,
        glyph_data: GlyphData {
            data: glyph_bytes,
            is_compressed,
        },
    })
}

pub fn decompress_glyph(lazy_glyph: &LazyGlyph, version: FontVersion) -> Glyph {
    let (seek_bits, backseek_nbyte) = match version {
        FontVersion::V1 => (10, 2),
        FontVersion::V0 => (3, 1),
    };

    let decompressed = lazy_glyph.glyph_data.decompress(seek_bits, backseek_nbyte);
    let (tw, th) = lazy_glyph.texture_size;
    let tw = tw as usize;
    let th = th as usize;

    match version {
        FontVersion::V1 => {
            let mut pos = 0;
            let mut mip_level_0 = Vec::new();
            let mut mip_level_1 = None;
            let mut mip_level_2 = None;
            let mut mip_level_3 = None;

            for level in 0..4 {
                let w = tw >> level;
                let h = th >> level;
                if w == 0 || h == 0 {
                    break;
                }

                let expected_size = w * h;
                if pos + expected_size > decompressed.len() {
                    break;
                }

                let level_data = decompressed[pos..pos + expected_size].to_vec();
                pos += expected_size;

                match level {
                    0 => mip_level_0 = level_data,
                    1 => mip_level_1 = Some(level_data),
                    2 => mip_level_2 = Some(level_data),
                    3 => mip_level_3 = Some(level_data),
                    _ => {}
                }
            }

            Glyph {
                info: lazy_glyph.info.clone(),
                mip_level_0,
                mip_level_1,
                mip_level_2,
                mip_level_3,
                width: tw as u32,
                height: th as u32,
            }
        }
        FontVersion::V0 => {
            // 4bpp to 8bpp conversion
            let stride = (tw + 1) / 2;
            let mut pixels = Vec::with_capacity(tw * th);

            for y in 0..th {
                let row_start = y * stride;
                for x in 0..tw {
                    let byte_idx = row_start + x / 2;
                    if byte_idx < decompressed.len() {
                        let byte_4bpp = decompressed[byte_idx];
                        let pixel = if x % 2 == 0 {
                            (byte_4bpp >> 4) << 4
                        } else {
                            (byte_4bpp & 0xF) << 4
                        };
                        pixels.push(pixel);
                    }
                }
            }

            Glyph {
                info: lazy_glyph.info.clone(),
                mip_level_0: pixels,
                mip_level_1: None,
                mip_level_2: None,
                mip_level_3: None,
                width: tw as u32,
                height: th as u32,
            }
        }
    }
}

fn save_glyph_to_png(glyph: &Glyph, output_path: &Path) -> std::io::Result<()> {
    let (aw, ah) = glyph.info.actual_size();
    let aw = aw as u32;
    let ah = ah as u32;

    if aw == 0 || ah == 0 {
        return Ok(());
    }

    let mut img = RgbaImage::new(aw, ah);

    for y in 0..ah {
        for x in 0..aw {
            let idx = (y * glyph.width + x) as usize;
            if idx < glyph.mip_level_0.len() {
                let alpha = glyph.mip_level_0[idx];
                img.put_pixel(x, y, Rgba([0, 0, 0, alpha]));
            }
        }
    }

    img.save(output_path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
}

pub fn detect_mipmap_levels(font: &Font) -> usize {
    if font.version == FontVersion::V0 {
        return 1;
    }

    let mut max_levels = 1usize;

    for lazy_glyph in font.glyphs.values().take(10) {
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

pub fn export_font(font: &Font, output_dir: &Path) -> std::io::Result<usize> {
    fs::create_dir_all(output_dir)?;

    let mipmap_levels = detect_mipmap_levels(font);
    let mut metadata = Vec::new();
    metadata.push(format!("ascent: {}", font.ascent));
    metadata.push(format!("descent: {}", font.descent));
    metadata.push("characters:".to_string());

    for (char_code, &glyph_id) in font.characters.iter().enumerate() {
        metadata.push(format!("  {:04x}: {:04}", char_code, glyph_id));
    }

    metadata.push("glyphs:".to_string());

    let mut sorted_glyphs: Vec<_> = font.glyphs.iter().collect();
    sorted_glyphs.sort_by_key(|(id, _)| *id);

    for (&glyph_id, lazy_glyph) in &sorted_glyphs {
        let info = &lazy_glyph.info;
        let code_label = match font.version {
            FontVersion::V1 => format!("unicode: {:04x}", info.char_code),
            FontVersion::V0 => format!("sjis: {:04x}", info.char_code),
        };
        metadata.push(format!("  {:04} {}", glyph_id, code_label));
        metadata.push(format!("    bearing_y: {}", info.bearing_y));
        metadata.push(format!("    bearing_x: {}", info.bearing_x));
        metadata.push(format!("    advance  : {}", info.advance_width));
    }

    let metadata_path = output_dir.join("metadata.txt");
    let mut file = File::create(metadata_path)?;
    file.write_all(metadata.join("\n").as_bytes())?;

    let total = sorted_glyphs.len();
    let counter = AtomicUsize::new(0);

    println!("Exporting {} glyphs in parallel...", total);

    sorted_glyphs
        .par_iter()
        .for_each(|(&glyph_id, lazy_glyph)| {
            let glyph = decompress_glyph(lazy_glyph, font.version);
            let info = &lazy_glyph.info;
            let filename = format!("{:04}_{:04x}_0.png", glyph_id, info.char_code);
            let glyph_path = output_dir.join(&filename);
            let _ = save_glyph_to_png(&glyph, &glyph_path);

            let done = counter.fetch_add(1, Ordering::Relaxed) + 1;
            if done % 100 == 0 || done == total {
                print!(
                    "\rExporting glyphs: {}/{} ({:.1}%)",
                    done,
                    total,
                    done as f64 / total as f64 * 100.0
                );
                std::io::stdout().flush().ok();
            }
        });

    println!();

    Ok(mipmap_levels)
}

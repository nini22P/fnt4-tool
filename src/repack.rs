use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use image::ImageReader;
use rayon::prelude::*;

use super::types::*;
use crate::lz77;

pub fn parse_metadata(metadata_path: &Path) -> std::io::Result<FntMetadata> {
    let file = File::open(metadata_path)?;
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().collect::<Result<_, _>>()?;

    let mut metadata = FntMetadata {
        ascent: 0,
        descent: 0,
        characters: HashMap::new(),
        glyphs: HashMap::new(),
    };

    let mut mode = "";
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();
        if line.is_empty() {
            i += 1;
            continue;
        }

        if line.starts_with("ascent:") {
            metadata.ascent = line.split(':').nth(1).unwrap().trim().parse().unwrap_or(0);
            i += 1;
        } else if line.starts_with("descent:") {
            metadata.descent = line.split(':').nth(1).unwrap().trim().parse().unwrap_or(0);
            i += 1;
        } else if line == "characters:" {
            mode = "characters";
            i += 1;
        } else if line == "glyphs:" {
            mode = "glyphs";
            i += 1;
        } else if mode == "characters" {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 2 {
                let char_code = u32::from_str_radix(parts[0].trim(), 16).unwrap_or(0);
                let glyph_id: u32 = parts[1].trim().parse().unwrap_or(0);
                metadata.characters.insert(char_code, glyph_id);
            }
            i += 1;
        } else if mode == "glyphs" {
            // Parse glyph block (4 lines)
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let glyph_id: u32 = parts[0].parse().unwrap_or(0);
                let code_type = parts[1].trim_end_matches(':').to_string();
                let char_code = u32::from_str_radix(parts[2], 16).unwrap_or(0);

                // Line 2: bearing_y
                let bearing_y: i8 = if i + 1 < lines.len() {
                    lines[i + 1]
                        .split(':')
                        .nth(1)
                        .unwrap_or("0")
                        .trim()
                        .parse()
                        .unwrap_or(0)
                } else {
                    0
                };

                // Line 3: bearing_x
                let bearing_x: i8 = if i + 2 < lines.len() {
                    lines[i + 2]
                        .split(':')
                        .nth(1)
                        .unwrap_or("0")
                        .trim()
                        .parse()
                        .unwrap_or(0)
                } else {
                    0
                };

                // Line 4: advance
                let advance: u8 = if i + 3 < lines.len() {
                    lines[i + 3]
                        .split(':')
                        .nth(1)
                        .unwrap_or("0")
                        .trim()
                        .parse()
                        .unwrap_or(0)
                } else {
                    0
                };

                metadata.glyphs.insert(
                    glyph_id,
                    GlyphMetadata {
                        char_code,
                        code_type,
                        bearing_x,
                        bearing_y,
                        advance,
                    },
                );

                i += 4;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    Ok(metadata)
}

fn ceil_power_of_2(n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    let mut p = 1u32;
    while p < n {
        p <<= 1;
    }
    p
}

fn process_single_glyph(
    input_dir: &Path,
    glyph_id: u32,
    glyph_info: &GlyphMetadata,
    fnt_version: FntVersion,
    mipmap_levels: usize,
) -> Option<(u32, ProcessedGlyph)> {
    let png_filename = format!("{:04}_{:04x}_0.png", glyph_id, glyph_info.char_code);
    let png_path = input_dir.join(&png_filename);

    if !png_path.exists() {
        return None;
    }

    let img = ImageReader::open(&png_path).ok()?.decode().ok()?;
    let rgba = img.to_rgba8();
    let actual_width = rgba.width() as u8;
    let actual_height = rgba.height() as u8;

    let alpha: Vec<u8> = rgba.pixels().map(|p| p.0[3]).collect();

    let (texture_width, texture_height, raw_pixel_data) = match fnt_version {
        FntVersion::V1 => {
            let tw = ceil_power_of_2(actual_width as u32) as u8;
            let th = ceil_power_of_2(actual_height as u32) as u8;

            let mut canvas = vec![0u8; (tw as usize) * (th as usize)];
            for y in 0..(actual_height as usize) {
                for x in 0..(actual_width as usize) {
                    canvas[y * (tw as usize) + x] = alpha[y * (actual_width as usize) + x];
                }
            }

            let mut mipmaps = vec![canvas];
            let mut w = tw as usize;
            let mut h = th as usize;

            for _ in 1..mipmap_levels {
                if w <= 1 && h <= 1 {
                    break;
                }
                if w > 1 && h > 1 {
                    let new_w = w / 2;
                    let new_h = h / 2;
                    let prev = mipmaps.last().unwrap();
                    let mut mip = vec![0u8; new_w * new_h];

                    for y in 0..new_h {
                        for x in 0..new_w {
                            let tl = prev[(y * 2) * w + (x * 2)] as u32;
                            let tr = prev[(y * 2) * w + (x * 2 + 1)] as u32;
                            let bl = prev[(y * 2 + 1) * w + (x * 2)] as u32;
                            let br = prev[(y * 2 + 1) * w + (x * 2 + 1)] as u32;
                            mip[y * new_w + x] = ((tl + tr + bl + br) / 4) as u8;
                        }
                    }
                    mipmaps.push(mip);
                    w = new_w;
                    h = new_h;
                } else {
                    break;
                }
            }

            let raw: Vec<u8> = mipmaps.into_iter().flatten().collect();
            (tw, th, raw)
        }
        FntVersion::V0 => unimplemented!("V0 repack not supported"),
    };

    let compressed_data = lz77::compress(&raw_pixel_data, 10);
    let (data_to_write, compressed_size) = if compressed_data.len() >= raw_pixel_data.len() {
        (raw_pixel_data, 0u16)
    } else {
        let len = compressed_data.len() as u16;
        (compressed_data, len)
    };

    Some((
        glyph_id,
        ProcessedGlyph {
            glyph_info: glyph_info.clone(),
            actual_width,
            actual_height,
            texture_width,
            texture_height,
            data_to_write,
            compressed_size,
        },
    ))
}

pub fn process_glyphs(
    input_dir: &Path,
    metadata: &FntMetadata,
    fnt_version: FntVersion,
    mipmap_levels: usize,
) -> std::io::Result<HashMap<u32, ProcessedGlyph>> {
    let mipmap_levels = mipmap_levels.clamp(1, 4);
    let mut glyph_ids: Vec<u32> = metadata.glyphs.keys().copied().collect();
    glyph_ids.sort();

    let total = glyph_ids.len();
    let counter = AtomicUsize::new(0);

    println!("Processing {} glyphs in parallel...", total);

    let results: Vec<_> = glyph_ids
        .par_iter()
        .filter_map(|&glyph_id| {
            let glyph_info = metadata.glyphs.get(&glyph_id)?;
            let result =
                process_single_glyph(input_dir, glyph_id, glyph_info, fnt_version, mipmap_levels);

            let done = counter.fetch_add(1, Ordering::Relaxed) + 1;
            if done % 100 == 0 || done == total {
                print!(
                    "\rProcessing glyphs: {}/{} ({:.1}%)",
                    done,
                    total,
                    done as f64 / total as f64 * 100.0
                );
                std::io::stdout().flush().ok();
            }

            result
        })
        .collect();

    println!();

    let processed_glyphs: HashMap<u32, ProcessedGlyph> = results.into_iter().collect();
    Ok(processed_glyphs)
}

pub fn repack_fnt(
    output_fnt: &Path,
    metadata: &FntMetadata,
    processed_glyphs: &HashMap<u32, ProcessedGlyph>,
) -> std::io::Result<()> {
    let header_size = 16usize;
    let character_table_size = 65536 * 4;

    let mut current_offset = header_size + character_table_size;
    let mut glyph_final_info: HashMap<u32, usize> = HashMap::new();
    let mut glyph_ids: Vec<u32> = processed_glyphs.keys().copied().collect();
    glyph_ids.sort();

    for &glyph_id in &glyph_ids {
        let glyph_data = &processed_glyphs[&glyph_id];
        glyph_final_info.insert(glyph_id, current_offset);
        current_offset += 10 + glyph_data.data_to_write.len();
    }

    let total_file_size = current_offset as u32;

    let default_glyph_id = *glyph_ids.first().unwrap_or(&0);
    let default_offset = *glyph_final_info
        .get(&default_glyph_id)
        .unwrap_or(&(header_size + character_table_size)) as u32;

    let mut char_table = vec![default_offset; 65536];
    for (&char_code, &glyph_id) in &metadata.characters {
        if let Some(&offset) = glyph_final_info.get(&glyph_id) {
            char_table[char_code as usize] = offset as u32;
        }
    }

    let mut file = File::create(output_fnt)?;

    let header = FntHeader {
        magic: *b"FNT4",
        version: FntVersion::V1,
        file_size: total_file_size,
        ascent: metadata.ascent,
        descent: metadata.descent,
    };
    file.write_all(&header.to_bytes())?;

    for offset in &char_table {
        file.write_all(&offset.to_le_bytes())?;
    }

    for &glyph_id in &glyph_ids {
        let glyph_data = &processed_glyphs[&glyph_id];
        let glyph_info = &glyph_data.glyph_info;

        let glyph_header = GlyphHeader {
            bearing_x: glyph_info.bearing_x,
            bearing_y: glyph_info.bearing_y,
            actual_width: glyph_data.actual_width,
            actual_height: glyph_data.actual_height,
            advance_width: glyph_info.advance,
            unused: 0,
            texture_width: glyph_data.texture_width,
            texture_height: glyph_data.texture_height,
            compressed_size: glyph_data.compressed_size,
        };

        file.write_all(&glyph_header.to_bytes_v1())?;
        file.write_all(&glyph_data.data_to_write)?;
    }

    Ok(())
}

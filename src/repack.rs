use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use image::ImageReader;
use rayon::prelude::*;

use super::types::*;
use crate::texture::encode_glyph_texture;

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

    let raw_pixels: Vec<u8> = rgba.pixels().map(|p| p.0[3]).collect();

    let encoded = match fnt_version {
        FntVersion::V1 => {
            encode_glyph_texture(&raw_pixels, actual_width, actual_height, mipmap_levels)
        }
        FntVersion::V0 => unimplemented!("FNT4 V0 repack not supported"),
    };

    Some((
        glyph_id,
        ProcessedGlyph {
            glyph_info: glyph_info.clone(),
            actual_width,
            actual_height,
            texture_width: encoded.texture_width,
            texture_height: encoded.texture_height,
            data_to_write: encoded.data,
            compressed_size: encoded.compressed_size,
        },
    ))
}

pub fn process_glyphs(
    input_dir: &Path,
    metadata: &FntMetadata,
    fnt_version: FntVersion,
) -> std::io::Result<BTreeMap<u32, ProcessedGlyph>> {
    let mipmap_levels = metadata.mipmap_levels;
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

    let processed_glyphs: BTreeMap<u32, ProcessedGlyph> = results.into_iter().collect();
    Ok(processed_glyphs)
}

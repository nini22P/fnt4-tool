use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use rayon::prelude::*;
use serde::{Deserialize, Deserializer, Serialize};

use crate::fnt::Fnt;
use crate::glyph::{GlyphInfo, ProcessedGlyph, RenderedGlyph, encode_glyph_texture};
use crate::metadata::{FntVersion, GlyphMetadata};
use crate::utils::downsample_lanczos;

fn default_size() -> Option<f32> {
    None
}

fn default_quality() -> u8 {
    1
}

fn default_padding() -> u8 {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebuildConfig {
    #[serde(default = "default_size")]
    pub size: Option<f32>,
    #[serde(default = "default_quality")]
    pub quality: u8,
    #[serde(default = "default_padding")]
    pub padding: u8,
    #[serde(default, deserialize_with = "deserialize_replace")]
    pub replace: BTreeMap<u32, char>,
}

impl RebuildConfig {
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let content = fs::read_to_string(path)?;
        let config: RebuildConfig = toml::from_str(&content).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("TOML parse error: {}", e),
            )
        })?;

        println!("Loaded {} replace entries.", config.replace.len());
        Ok(config)
    }
}

impl Default for RebuildConfig {
    fn default() -> Self {
        RebuildConfig {
            size: default_size(),
            quality: default_quality(),
            padding: default_padding(),
            replace: BTreeMap::new(),
        }
    }
}

pub fn rebuild_fnt(
    fnt: Fnt,
    output_fnt: &Path,
    source_font: &Path,
    config: &RebuildConfig,
) -> std::io::Result<()> {
    if fnt.metadata.version != FntVersion::V1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Rebuild only supports FNT4 V1 format",
        ));
    }

    let font_data = std::fs::read(source_font)?;
    let font = FontRef::try_from_slice(&font_data).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Failed to parse TTF/OTF font: {:?}", e),
        )
    })?;

    let mut processed_glyphs = process_glyphs_from_source_font(&fnt, &font, &config)?;

    let mut restored_count = 0;
    for (glyph_id, processed_glyph) in processed_glyphs.iter_mut() {
        if processed_glyph.actual_width == 0 || processed_glyph.actual_height == 0 {
            if let Some(original_glyph) = fnt.lazy_glyphs.get(glyph_id) {
                let compressed_size = if original_glyph.glyph_data.is_compressed {
                    original_glyph.glyph_data.data.len() as u16
                } else {
                    0
                };

                *processed_glyph = ProcessedGlyph {
                    glyph_info: fnt.metadata.glyphs[glyph_id].clone(),
                    actual_width: original_glyph.info.actual_width,
                    actual_height: original_glyph.info.actual_height,
                    texture_width: original_glyph.info.texture_width,
                    texture_height: original_glyph.info.texture_height,
                    data: original_glyph.glyph_data.data.clone(),
                    compressed_size,
                };

                restored_count += 1;

                println!(
                    "Restored glyph ID: {} (U+{:04X}) from original fnt",
                    glyph_id, fnt.metadata.glyphs[glyph_id].char_code
                );
            }
        }
    }

    if restored_count > 0 {
        println!(
            "Fallback Summary: Restored {} glyphs from original fnt (missing or empty in TTF/OTF).",
            restored_count
        );
    }

    let new_fnt = Fnt::from_processed_glyphs(fnt.metadata, processed_glyphs);

    new_fnt.write_fnt(output_fnt)?;

    println!("Successfully rebuilt to {:?}", output_fnt);
    Ok(())
}

fn render_glyph_from_source_font<F: Font>(
    font: &F,
    character: char,
    font_size: f32,
    quality: u8,
) -> Option<RenderedGlyph> {
    let glyph_id = font.glyph_id(character);
    if glyph_id.0 == 0 && character != '\0' {
        return None;
    }

    let ss = quality.max(1) as f32;
    let render_size = font_size * ss;

    let scale = PxScale::from(render_size);
    let target_scale = PxScale::from(font_size);
    let target_scaled_font = font.as_scaled(target_scale);

    let h_advance = target_scaled_font.h_advance(glyph_id);
    let glyph = glyph_id.with_scale(scale);

    let outlined = font.outline_glyph(glyph);

    if let Some(outlined) = outlined {
        let bounds = outlined.px_bounds();
        let hi_width = bounds.width().ceil() as u32;
        let hi_height = bounds.height().ceil() as u32;

        if hi_width == 0 || hi_height == 0 {
            return Some(RenderedGlyph {
                bearing_x: 0,
                bearing_y: 0,
                advance: h_advance.round() as u8,
                actual_width: 0,
                actual_height: 0,
                raw_pixels: vec![],
            });
        }

        let mut hi_pixels = vec![0.0f32; (hi_width * hi_height) as usize];
        outlined.draw(|x, y, c| {
            let idx = (y * hi_width + x) as usize;
            if idx < hi_pixels.len() {
                hi_pixels[idx] = c;
            }
        });

        let dst_width = ((hi_width as f32 / ss).ceil() as u32).max(1);
        let dst_height = ((hi_height as f32 / ss).ceil() as u32).max(1);

        let downsampled = if ss > 1.0 {
            let hi_u8: Vec<u8> = hi_pixels
                .iter()
                .map(|&c| (c * 255.0).clamp(0.0, 255.0) as u8)
                .collect();
            let down = downsample_lanczos(&hi_u8, hi_width, hi_height, dst_width, dst_height);
            down.iter().map(|&v| v as f32 / 255.0).collect::<Vec<_>>()
        } else {
            hi_pixels
        };

        let final_pixels: Vec<u8> = downsampled
            .iter()
            .map(|&c| (c * 255.0).round() as u8)
            .collect();

        let bearing_x = (bounds.min.x / ss).round() as i8;
        let bearing_y = ((-bounds.min.y) / ss).round() as i8;

        Some(RenderedGlyph {
            bearing_x,
            bearing_y,
            advance: h_advance.round().max(0.0).min(255.0) as u8,
            actual_width: dst_width.min(255) as u8,
            actual_height: dst_height.min(255) as u8,
            raw_pixels: final_pixels,
        })
    } else {
        Some(RenderedGlyph {
            bearing_x: 0,
            bearing_y: 0,
            advance: h_advance.round().max(0.0).min(255.0) as u8,
            actual_width: 0,
            actual_height: 0,
            raw_pixels: vec![],
        })
    }
}

fn process_single_glyph_from_source_font<F: Font>(
    font: &F,
    glyph_meta: &GlyphMetadata,
    original_glyph_info: &GlyphInfo,
    mipmap_level: usize,
    config: &RebuildConfig,
) -> Option<ProcessedGlyph> {
    let original_code = glyph_meta.char_code;
    let font_size = config.size?;

    let replaced_char = config.replace.get(&original_code);
    let (target_char, _is_replaced) = match replaced_char {
        Some(&c) => (c, true),
        None => (char::from_u32(original_code)?, false),
    };

    let rendered = render_glyph_from_source_font(font, target_char, font_size, config.quality);

    let (bearing_x, bearing_y, advance, actual_width, actual_height, raw_pixels) =
        if let Some(r) = rendered {
            (
                r.bearing_x,
                r.bearing_y,
                r.advance,
                r.actual_width,
                r.actual_height,
                r.raw_pixels,
            )
        } else {
            (
                original_glyph_info.bearing_x,
                original_glyph_info.bearing_y,
                original_glyph_info.advance,
                0u8,
                0u8,
                vec![],
            )
        };

    if actual_width == 0 || actual_height == 0 {
        let mut new_meta = glyph_meta.clone();
        new_meta.bearing_x = bearing_x;
        new_meta.bearing_y = bearing_y;
        new_meta.advance = advance;

        return Some(ProcessedGlyph {
            glyph_info: new_meta,
            actual_width: 0,
            actual_height: 0,
            texture_width: 0,
            texture_height: 0,
            data: vec![],
            compressed_size: 0,
        });
    }

    let (padded_width, padded_height, padded_data, padded_bearing_x, padded_bearing_y, new_advance) =
        if config.padding > 0 {
            let p = config.padding as usize;
            let new_w = actual_width as usize + p * 2;
            let new_h = actual_height as usize + p * 2;
            let mut padded = vec![0u8; new_w * new_h];

            for y in 0..actual_height as usize {
                for x in 0..actual_width as usize {
                    let src_idx = y * actual_width as usize + x;
                    let dst_idx = (y + p) * new_w + (x + p);
                    if src_idx < raw_pixels.len() {
                        padded[dst_idx] = raw_pixels[src_idx];
                    }
                }
            }

            let new_bearing_x = bearing_x.saturating_sub(config.padding as i8);
            let new_bearing_y = bearing_y.saturating_add(config.padding as i8);

            let new_advance = advance.saturating_add(config.padding * 2);

            (
                new_w.min(255) as u8,
                new_h.min(255) as u8,
                padded,
                new_bearing_x,
                new_bearing_y,
                new_advance.min(255),
            )
        } else {
            (
                actual_width,
                actual_height,
                raw_pixels,
                bearing_x,
                bearing_y,
                advance,
            )
        };

    let mut new_meta = glyph_meta.clone();
    new_meta.bearing_x = padded_bearing_x;
    new_meta.bearing_y = padded_bearing_y;
    new_meta.advance = new_advance;

    create_processed_glyph(
        &new_meta,
        padded_width,
        padded_height,
        &padded_data,
        mipmap_level,
    )
}

fn create_processed_glyph(
    glyph_meta: &GlyphMetadata,
    actual_width: u8,
    actual_height: u8,
    raw_pixels: &[u8],
    mipmap_level: usize,
) -> Option<ProcessedGlyph> {
    let encoded = encode_glyph_texture(raw_pixels, actual_width, actual_height, mipmap_level);

    Some(ProcessedGlyph {
        glyph_info: glyph_meta.clone(),
        actual_width,
        actual_height,
        texture_width: encoded.texture_width,
        texture_height: encoded.texture_height,
        data: encoded.data,
        compressed_size: encoded.compressed_size,
    })
}

fn process_glyphs_from_source_font<F: Font + Sync>(
    fnt: &Fnt,
    font: &F,
    config: &RebuildConfig,
) -> std::io::Result<BTreeMap<u32, ProcessedGlyph>> {
    let metadata = fnt.metadata.clone();
    let mipmap_level = metadata.mipmap_level;
    let mut glyph_ids: Vec<u32> = metadata.glyphs.keys().copied().collect();
    glyph_ids.sort();

    let total = glyph_ids.len();
    let counter = AtomicUsize::new(0);

    println!(
        "Processing {} glyphs (size={:.1?}, quality={}x, padding={})...",
        total, config.size, config.quality, config.padding
    );

    let results: Vec<_> = glyph_ids
        .par_iter()
        .filter_map(|&glyph_id| {
            let glyph_meta = metadata.glyphs.get(&glyph_id)?;
            let lazy_glyph = fnt.lazy_glyphs.get(&glyph_id)?;

            let result = process_single_glyph_from_source_font(
                font,
                glyph_meta,
                &lazy_glyph.info,
                mipmap_level,
                config,
            );

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

            result.map(|pg| (glyph_id, pg))
        })
        .collect();

    println!();

    Ok(results.into_iter().collect())
}

fn deserialize_replace<'de, D>(deserializer: D) -> Result<BTreeMap<u32, char>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw_map: BTreeMap<String, String> = Deserialize::deserialize(deserializer)?;

    let mut replace = BTreeMap::new();

    for (raw_char_str, target_char_str) in raw_map {
        let src_char = raw_char_str.chars().next().unwrap();
        let src_code = src_char as u32;

        let target_char = target_char_str.chars().next().unwrap();

        replace.insert(src_code, target_char);
    }

    Ok(replace)
}

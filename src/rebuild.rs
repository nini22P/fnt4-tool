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
use crate::metadata::{CodeType, FntVersion, GlyphMetadata};
use crate::utils::{decode_sjis_u32, downsample_lanczos};

fn default_size() -> Option<f32> {
    None
}

fn default_quality() -> u8 {
    1
}

fn default_letter_spacing() -> i8 {
    0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebuildConfig {
    #[serde(default = "default_size")]
    pub size: Option<f32>,
    #[serde(default = "default_quality")]
    pub quality: u8,
    #[serde(default = "default_letter_spacing")]
    pub letter_spacing: i8,
    #[serde(default)]
    pub texture_padding: Option<u8>,
    #[serde(default, deserialize_with = "deserialize_replace")]
    pub replace: BTreeMap<u32, char>,
}

impl Default for RebuildConfig {
    fn default() -> Self {
        RebuildConfig {
            size: default_size(),
            quality: default_quality(),
            texture_padding: None,
            letter_spacing: default_letter_spacing(),
            replace: BTreeMap::new(),
        }
    }
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

struct ResolvedConfig {
    size: f32,
    quality: u8,
    texture_padding: u8,
    letter_spacing: i8,
    replace: BTreeMap<u32, char>,
}

pub fn rebuild_fnt(
    fnt: Fnt,
    output_fnt: &Path,
    source_font: &Path,
    config: &RebuildConfig,
) -> std::io::Result<()> {
    let font_size = if let Some(size) = config.size {
        size
    } else {
        let original_height =
            (fnt.metadata.ascent as i16 + fnt.metadata.descent as i16).unsigned_abs() as f32;
        println!(
            "Auto-calculated font size: {:.1} (ascent={}, descent={})",
            original_height, fnt.metadata.ascent, fnt.metadata.descent
        );
        original_height
    };

    let texture_padding = if let Some(padding) = config.texture_padding {
        padding
    } else {
        let padding = (1 << fnt.metadata.mipmap_level.saturating_sub(1)).max(4);
        println!(
            "Auto-calculated texture padding: {} (based on mipmap level {})",
            padding, fnt.metadata.mipmap_level
        );
        padding as u8
    };

    let resolved_config = ResolvedConfig {
        size: font_size,
        quality: config.quality,
        texture_padding: texture_padding,
        letter_spacing: config.letter_spacing,
        replace: config.replace.clone(),
    };

    let font_data = std::fs::read(source_font)?;
    let font = FontRef::try_from_slice(&font_data).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Failed to parse TTF/OTF font: {:?}", e),
        )
    })?;

    let mut processed_glyphs = process_glyphs_from_source_font(&fnt, &font, &resolved_config)?;

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

                let code_type = fnt.metadata.glyphs[glyph_id].code_type;
                let original_code = fnt.metadata.glyphs[glyph_id].char_code;
                let original_char = match code_type {
                    CodeType::Unicode => char::from_u32(original_code).unwrap_or(' '),
                    CodeType::Sjis => decode_sjis_u32(original_code).unwrap_or(' '),
                };

                match resolved_config.replace.get(&original_code) {
                    Some(&target_char) => {
                        println!(
                            "Restored glyph ID: {} ({:?} 0x{:04X} '{}' -> '{}') from original fnt",
                            glyph_id, code_type, original_code, original_char, target_char
                        );
                    }
                    None => {
                        println!(
                            "Restored glyph ID: {} ({:?} 0x{:04X} '{}') from original fnt",
                            glyph_id, code_type, original_code, original_char
                        );
                    }
                }
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

fn process_glyphs_from_source_font<F: Font + Sync>(
    fnt: &Fnt,
    font: &F,
    config: &ResolvedConfig,
) -> std::io::Result<BTreeMap<u32, ProcessedGlyph>> {
    let metadata = fnt.metadata.clone();
    let mipmap_level = metadata.mipmap_level;
    let mut glyph_ids: Vec<u32> = metadata.glyphs.keys().copied().collect();
    glyph_ids.sort();

    let total = glyph_ids.len();
    let counter = AtomicUsize::new(0);

    println!(
        "Processing {} glyphs (size={}, quality={}x, letter_spacing={}, texture_padding={})...",
        total, config.size, config.quality, config.letter_spacing, config.texture_padding
    );

    let results: Vec<_> = glyph_ids
        .par_iter()
        .filter_map(|&glyph_id| {
            let glyph_metadata = metadata.glyphs.get(&glyph_id)?;
            let lazy_glyph = fnt.lazy_glyphs.get(&glyph_id)?;

            let result = process_single_glyph_from_source_font(
                font,
                glyph_metadata,
                &lazy_glyph.info,
                mipmap_level,
                config,
                fnt.metadata.version,
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

fn process_single_glyph_from_source_font<F: Font>(
    font: &F,
    glyph_metadata: &GlyphMetadata,
    original_glyph_info: &GlyphInfo,
    mipmap_level: usize,
    config: &ResolvedConfig,
    fnt_version: FntVersion,
) -> Option<ProcessedGlyph> {
    let original_code = glyph_metadata.char_code;
    let code_type = glyph_metadata.code_type;
    let font_size = config.size;

    let replaced_char = config.replace.get(&original_code);
    let (target_char, _is_replaced) = match replaced_char {
        Some(&c) => (c, true),
        None => match code_type {
            CodeType::Unicode => (char::from_u32(original_code)?, false),
            CodeType::Sjis => match decode_sjis_u32(original_code) {
                Some(c) => (c, false),
                None => {
                    println!(
                        "Failed to decode SJIS to Unicode: (U+{:04X})",
                        original_code
                    );
                    (char::from_u32(0)?, false)
                }
            },
        },
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
        let new_advance = (advance as i16 + config.letter_spacing as i16)
            .max(0)
            .min(255) as u8;

        let mut new_metadata = glyph_metadata.clone();
        new_metadata.bearing_x = bearing_x;
        new_metadata.bearing_y = bearing_y;
        new_metadata.advance = new_advance;

        return Some(ProcessedGlyph {
            glyph_info: new_metadata,
            actual_width: 0,
            actual_height: 0,
            texture_width: 0,
            texture_height: 0,
            data: vec![],
            compressed_size: 0,
        });
    }

    let tp = config.texture_padding as usize;

    let align_shift = mipmap_level.max(2); // make sure at least 4 bytes aligned
    let align_mask = (1 << align_shift) - 1;

    let align_up = |val: usize| -> usize { (val + align_mask) & !align_mask };

    let (final_width, final_height, final_data, final_bearing_x, final_bearing_y, final_advance) =
        if tp > 0 {
            let padded_w = actual_width as usize + tp * 2;
            let padded_h = actual_height as usize + tp * 2;

            let aligned_w = align_up(padded_w);
            let aligned_h = align_up(padded_h);

            let final_w = aligned_w.min(255);
            let final_h = aligned_h.min(255);

            let mut buffer = vec![0u8; final_w * final_h];

            for y in 0..actual_height as usize {
                for x in 0..actual_width as usize {
                    if x + tp < final_w && y + tp < final_h {
                        let src_idx = y * actual_width as usize + x;
                        let dst_idx = (y + tp) * final_w + (x + tp);

                        if src_idx < raw_pixels.len() {
                            buffer[dst_idx] = raw_pixels[src_idx];
                        }
                    }
                }
            }

            let new_bearing_x = bearing_x.saturating_sub(config.texture_padding as i8);
            let new_bearing_y = bearing_y.saturating_add(config.texture_padding as i8);

            let calc_advance = advance as i16 + config.letter_spacing as i16;
            let new_advance = calc_advance.max(0).min(255) as u8;

            (
                final_w as u8,
                final_h as u8,
                buffer,
                new_bearing_x,
                new_bearing_y,
                new_advance,
            )
        } else {
            let aligned_w = align_up(actual_width as usize);
            let aligned_h = align_up(actual_height as usize);

            let final_w = aligned_w.min(255);
            let final_h = aligned_h.min(255);

            let buffer = if final_w != actual_width as usize || final_h != actual_height as usize {
                let mut buf = vec![0u8; final_w * final_h];
                for y in 0..actual_height as usize {
                    for x in 0..actual_width as usize {
                        let src_idx = y * actual_width as usize + x;
                        let dst_idx = y * final_w + x;
                        if src_idx < raw_pixels.len() {
                            buf[dst_idx] = raw_pixels[src_idx];
                        }
                    }
                }
                buf
            } else {
                raw_pixels.to_vec()
            };

            let calc_advance = advance as i16 + config.letter_spacing as i16;
            let new_advance = calc_advance.max(0).min(255) as u8;

            (
                final_w as u8,
                final_h as u8,
                buffer,
                bearing_x,
                bearing_y,
                new_advance,
            )
        };

    let mut new_metadata = glyph_metadata.clone();
    new_metadata.bearing_x = final_bearing_x;
    new_metadata.bearing_y = final_bearing_y;
    new_metadata.advance = final_advance;

    create_processed_glyph(
        &new_metadata,
        final_width,
        final_height,
        &final_data,
        mipmap_level,
        fnt_version,
    )
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

fn create_processed_glyph(
    glyph_metadata: &GlyphMetadata,
    actual_width: u8,
    actual_height: u8,
    data: &[u8],
    mipmap_level: usize,
    fnt_version: FntVersion,
) -> Option<ProcessedGlyph> {
    let encoded =
        encode_glyph_texture(data, actual_width, actual_height, mipmap_level, fnt_version);

    Some(ProcessedGlyph {
        glyph_info: glyph_metadata.clone(),
        actual_width,
        actual_height,
        texture_width: encoded.texture_width,
        texture_height: encoded.texture_height,
        data: encoded.data,
        compressed_size: encoded.compressed_size,
    })
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

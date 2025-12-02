use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use rayon::prelude::*;

use crate::texture::encode_glyph_texture;
use crate::types::{
    Fnt, FntMetadata, FntVersion, GlyphMetadata, ProcessedGlyph, RebuildConfig, RenderedGlyph,
};

pub fn rebuild_fnt(
    fnt: Fnt,
    output_fnt: &Path,
    source_font: &Path,
    config: &RebuildConfig,
) -> std::io::Result<()> {
    if fnt.version != FntVersion::V1 {
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

    let metadata = fnt.extract_metadata();

    let mut processed_glyphs = process_glyphs_from_source_font(&fnt, &metadata, &font, &config)?;

    let mut restored_count = 0;
    for (glyph_id, processed_glyph) in processed_glyphs.iter_mut() {
        if processed_glyph.actual_width == 0 || processed_glyph.actual_height == 0 {
            if let Some(original_glyph) = fnt.glyphs.get(glyph_id) {
                let compressed_size = if original_glyph.glyph_data.is_compressed {
                    original_glyph.glyph_data.data.len() as u16
                } else {
                    0
                };

                *processed_glyph = ProcessedGlyph {
                    glyph_info: metadata.glyphs[glyph_id].clone(),
                    actual_width: original_glyph.info.actual_width,
                    actual_height: original_glyph.info.actual_height,
                    texture_width: original_glyph.info.texture_width,
                    texture_height: original_glyph.info.texture_height,
                    data_to_write: original_glyph.glyph_data.data.clone(),
                    compressed_size,
                };

                restored_count += 1;

                println!(
                    "Restored glyph ID: {} (U+{:04X}) from original fnt",
                    glyph_id, metadata.glyphs[glyph_id].char_code
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

    let new_fnt = Fnt::from_processed_data(metadata, processed_glyphs, FntVersion::V1);

    new_fnt.save_fnt(output_fnt)?;

    println!("Successfully rebuilt to {:?}", output_fnt);
    Ok(())
}

fn downsample_lanczos(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    if src_w == dst_w && src_h == dst_h {
        return src.to_vec();
    }

    let mut dst = vec![0u8; (dst_w * dst_h) as usize];
    let scale_x = src_w as f64 / dst_w as f64;
    let scale_y = src_h as f64 / dst_h as f64;

    let is_integer_scale = (scale_x.round() - scale_x).abs() < 0.001
        && (scale_y.round() - scale_y).abs() < 0.001
        && scale_x >= 1.0
        && scale_y >= 1.0;

    if is_integer_scale {
        let sx = scale_x.round() as u32;
        let sy = scale_y.round() as u32;
        let area = (sx * sy) as f64;

        for dy in 0..dst_h {
            for dx in 0..dst_w {
                let mut sum = 0.0;
                for oy in 0..sy {
                    for ox in 0..sx {
                        let x = dx * sx + ox;
                        let y = dy * sy + oy;
                        if x < src_w && y < src_h {
                            sum += src[(y * src_w + x) as usize] as f64;
                        }
                    }
                }
                dst[(dy * dst_w + dx) as usize] = (sum / area).round() as u8;
            }
        }
    } else {
        let a = 3.0;

        for dy in 0..dst_h {
            for dx in 0..dst_w {
                let src_x = (dx as f64 + 0.5) * scale_x - 0.5;
                let src_y = (dy as f64 + 0.5) * scale_y - 0.5;

                let x0 = (src_x - a).floor().max(0.0) as u32;
                let x1 = (src_x + a).ceil().min(src_w as f64 - 1.0) as u32;
                let y0 = (src_y - a).floor().max(0.0) as u32;
                let y1 = (src_y + a).ceil().min(src_h as f64 - 1.0) as u32;

                let mut sum = 0.0;
                let mut weight_sum = 0.0;

                for sy in y0..=y1 {
                    for sx in x0..=x1 {
                        let wx = lanczos_weight(sx as f64 - src_x, a);
                        let wy = lanczos_weight(sy as f64 - src_y, a);
                        let w = wx * wy;
                        sum += src[(sy * src_w + sx) as usize] as f64 * w;
                        weight_sum += w;
                    }
                }

                let value = if weight_sum > 0.0 {
                    (sum / weight_sum).round().clamp(0.0, 255.0) as u8
                } else {
                    0
                };
                dst[(dy * dst_w + dx) as usize] = value;
            }
        }
    }

    dst
}

fn lanczos_weight(x: f64, a: f64) -> f64 {
    if x.abs() < 1e-10 {
        1.0
    } else if x.abs() >= a {
        0.0
    } else {
        let pi_x = std::f64::consts::PI * x;
        let pi_x_a = pi_x / a;
        (pi_x.sin() / pi_x) * (pi_x_a.sin() / pi_x_a)
    }
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
                advance_width: h_advance.round() as u8,
                actual_width: 0,
                actual_height: 0,
                alpha_data: vec![],
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
            advance_width: h_advance.round().max(0.0).min(255.0) as u8,
            actual_width: dst_width.min(255) as u8,
            actual_height: dst_height.min(255) as u8,
            alpha_data: final_pixels,
        })
    } else {
        Some(RenderedGlyph {
            bearing_x: 0,
            bearing_y: 0,
            advance_width: h_advance.round().max(0.0).min(255.0) as u8,
            actual_width: 0,
            actual_height: 0,
            alpha_data: vec![],
        })
    }
}

fn process_single_glyph_from_source_font<F: Font>(
    font: &F,
    _glyph_id: u32,
    glyph_meta: &GlyphMetadata,
    original_info: &crate::types::GlyphInfo,
    mipmap_levels: usize,
    config: &RebuildConfig,
) -> Option<ProcessedGlyph> {
    let original_code = glyph_meta.char_code;
    let font_size = config.size?;

    let hijacked_char = config.hijack_map.get(&original_code);
    let (target_char, _is_hijacked) = match hijacked_char {
        Some(&c) => (c, true),
        None => (char::from_u32(original_code)?, false),
    };

    let rendered = render_glyph_from_source_font(font, target_char, font_size, config.quality);

    let (bearing_x, bearing_y, advance_width, actual_width, actual_height, alpha_data) =
        if let Some(r) = rendered {
            (
                r.bearing_x,
                r.bearing_y,
                r.advance_width,
                r.actual_width,
                r.actual_height,
                r.alpha_data,
            )
        } else {
            (
                original_info.bearing_x,
                original_info.bearing_y,
                original_info.advance_width,
                0u8,
                0u8,
                vec![],
            )
        };

    if actual_width == 0 || actual_height == 0 {
        let mut new_meta = glyph_meta.clone();
        new_meta.bearing_x = bearing_x;
        new_meta.bearing_y = bearing_y;
        new_meta.advance_width = advance_width;

        return Some(ProcessedGlyph {
            glyph_info: new_meta,
            actual_width: 0,
            actual_height: 0,
            texture_width: 0,
            texture_height: 0,
            data_to_write: vec![],
            compressed_size: 0,
        });
    }

    let (
        padded_width,
        padded_height,
        padded_data,
        padded_bearing_x,
        padded_bearing_y,
        new_advance_width,
    ) = if config.padding > 0 {
        let p = config.padding as usize;
        let new_w = actual_width as usize + p * 2;
        let new_h = actual_height as usize + p * 2;
        let mut padded = vec![0u8; new_w * new_h];

        for y in 0..actual_height as usize {
            for x in 0..actual_width as usize {
                let src_idx = y * actual_width as usize + x;
                let dst_idx = (y + p) * new_w + (x + p);
                if src_idx < alpha_data.len() {
                    padded[dst_idx] = alpha_data[src_idx];
                }
            }
        }

        let new_bearing_x = bearing_x.saturating_sub(config.padding as i8);
        let new_bearing_y = bearing_y.saturating_add(config.padding as i8);

        let new_advance_width = advance_width.saturating_add(config.padding * 2);

        (
            new_w.min(255) as u8,
            new_h.min(255) as u8,
            padded,
            new_bearing_x,
            new_bearing_y,
            new_advance_width.min(255),
        )
    } else {
        (
            actual_width,
            actual_height,
            alpha_data,
            bearing_x,
            bearing_y,
            advance_width,
        )
    };

    let mut new_meta = glyph_meta.clone();
    new_meta.bearing_x = padded_bearing_x;
    new_meta.bearing_y = padded_bearing_y;
    new_meta.advance_width = new_advance_width;

    create_processed_glyph(
        &new_meta,
        padded_width,
        padded_height,
        &padded_data,
        mipmap_levels,
    )
}

fn create_processed_glyph(
    glyph_meta: &GlyphMetadata,
    actual_width: u8,
    actual_height: u8,
    raw_pixels: &[u8],
    mipmap_levels: usize,
) -> Option<ProcessedGlyph> {
    let encoded = encode_glyph_texture(raw_pixels, actual_width, actual_height, mipmap_levels);

    Some(ProcessedGlyph {
        glyph_info: glyph_meta.clone(),
        actual_width,
        actual_height,
        texture_width: encoded.texture_width,
        texture_height: encoded.texture_height,
        data_to_write: encoded.data,
        compressed_size: encoded.compressed_size,
    })
}

fn process_glyphs_from_source_font<F: Font + Sync>(
    fnt: &Fnt,
    metadata: &FntMetadata,
    font: &F,
    config: &RebuildConfig,
) -> std::io::Result<BTreeMap<u32, ProcessedGlyph>> {
    let mipmap_levels = metadata.mipmap_levels;
    let mut glyph_ids: Vec<u32> = metadata.glyphs.keys().copied().collect();
    glyph_ids.sort();

    let total = glyph_ids.len();
    let counter = AtomicUsize::new(0);

    println!(
        "Processing {} glyphs (size={:.1?}px, quality={}x, padding={})...",
        total, config.size, config.quality, config.padding
    );

    let results: Vec<_> = glyph_ids
        .par_iter()
        .filter_map(|&glyph_id| {
            let glyph_meta = metadata.glyphs.get(&glyph_id)?;
            let lazy_glyph = fnt.glyphs.get(&glyph_id)?;

            let result = process_single_glyph_from_source_font(
                font,
                glyph_id,
                glyph_meta,
                &lazy_glyph.info,
                mipmap_levels,
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

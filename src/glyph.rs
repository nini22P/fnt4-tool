use std::collections::BTreeMap;

use crate::{
    lz77,
    metadata::{FntVersion, GlyphMetadata},
    utils::ceil_power_of_2,
};

#[derive(Debug, Clone)]
pub struct GlyphHeader {
    pub bearing_x: i8,
    pub bearing_y: i8,
    pub actual_width: u8,
    pub actual_height: u8,
    pub advance: u8,
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
                    advance: data[offset + 4],
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
                    advance: data[offset + 4],
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
        result.push(self.advance);
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
    pub advance: u8,
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
            advance: header.advance,
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

#[derive(Debug)]
pub struct Glyph {
    pub info: GlyphInfo,
    pub mipmap: BTreeMap<u8, Vec<u8>>,
    pub width: u32,
    pub height: u32,
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
pub struct ProcessedGlyph {
    pub glyph_info: GlyphMetadata,
    pub actual_width: u8,
    pub actual_height: u8,
    pub texture_width: u8,
    pub texture_height: u8,
    pub data: Vec<u8>,
    pub compressed_size: u16,
}

pub struct RenderedGlyph {
    pub bearing_x: i8,
    pub bearing_y: i8,
    pub advance: u8,
    pub actual_width: u8,
    pub actual_height: u8,
    pub raw_pixels: Vec<u8>,
}

pub struct EncodedTexture {
    pub texture_width: u8,
    pub texture_height: u8,
    pub data: Vec<u8>,
    pub compressed_size: u16,
}

pub fn encode_glyph_texture(
    raw_pixels: &[u8],
    actual_width: u8,
    actual_height: u8,
    mipmap_level: usize,
) -> EncodedTexture {
    if actual_width == 0 || actual_height == 0 {
        return EncodedTexture {
            texture_width: 0,
            texture_height: 0,
            data: vec![],
            compressed_size: 0,
        };
    }

    let texture_width = ceil_power_of_2(actual_width as u32) as u8;
    let texture_height = ceil_power_of_2(actual_height as u32) as u8;

    let mut canvas = vec![0u8; (texture_width as usize) * (texture_height as usize)];
    for y in 0..(actual_height as usize) {
        for x in 0..(actual_width as usize) {
            let src_idx = y * (actual_width as usize) + x;
            let dst_idx = y * (texture_width as usize) + x;
            if src_idx < raw_pixels.len() {
                canvas[dst_idx] = raw_pixels[src_idx];
            }
        }
    }

    let mut mipmaps = vec![canvas];
    let mut w = texture_width as usize;
    let mut h = texture_height as usize;

    for _ in 1..mipmap_level {
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

    let raw_combined_data: Vec<u8> = mipmaps.into_iter().flatten().collect();

    let compressed_data = lz77::compress(&raw_combined_data, 10);
    let (data, compressed_size) = if compressed_data.len() >= raw_combined_data.len() {
        (raw_combined_data, 0u16)
    } else {
        let len = compressed_data.len() as u16;
        (compressed_data, len)
    };

    EncodedTexture {
        texture_width,
        texture_height,
        data,
        compressed_size,
    }
}

impl LazyGlyph {
    pub fn from_data(
        data: &[u8],
        offset: usize,
        char_code: u32,
        version: FntVersion,
    ) -> Result<LazyGlyph, &'static str> {
        let glyph_header = GlyphHeader::parse(data, offset, version)?;
        let compressed_size = glyph_header.compressed_size;

        let (texture_size, uncompressed_size) = match version {
            FntVersion::V1 => {
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
            FntVersion::V0 => {
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
}

impl Glyph {
    pub fn from_lazy_glyph(lazy_glyph: &LazyGlyph, version: FntVersion) -> Glyph {
        let (seek_bits, backseek_nbyte) = match version {
            FntVersion::V1 => (10, 2),
            FntVersion::V0 => (3, 1),
        };

        let decompressed = lazy_glyph.glyph_data.decompress(seek_bits, backseek_nbyte);
        let (tw, th) = lazy_glyph.texture_size;
        let tw = tw as usize;
        let th = th as usize;

        match version {
            FntVersion::V1 => {
                let mut pos = 0;
                let mut mipmap: BTreeMap<u8, Vec<u8>> = BTreeMap::new();

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

                    mipmap.insert(level as u8, level_data);
                }

                Glyph {
                    info: lazy_glyph.info.clone(),
                    mipmap,
                    width: tw as u32,
                    height: th as u32,
                }
            }
            FntVersion::V0 => {
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
                    mipmap: vec![(0, pixels)].into_iter().collect(),
                    width: tw as u32,
                    height: th as u32,
                }
            }
        }
    }
}

impl Glyph {
    pub fn write_png(&self, output_path: &std::path::Path) -> std::io::Result<()> {
        let (aw, ah) = self.info.actual_size();
        let aw = aw as u32;
        let ah = ah as u32;

        if aw == 0 || ah == 0 {
            return Ok(());
        }

        let mut img = image::RgbaImage::new(aw, ah);

        for y in 0..ah {
            for x in 0..aw {
                let idx = (y * self.width + x) as usize;
                if idx < self.mipmap[&0].len() {
                    let alpha = self.mipmap[&0][idx];
                    img.put_pixel(x, y, image::Rgba([0, 0, 0, alpha]));
                }
            }
        }

        img.save(output_path)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }
}

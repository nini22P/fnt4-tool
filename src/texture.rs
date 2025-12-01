use crate::lz77;

pub fn ceil_power_of_2(n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    let mut p = 1u32;
    while p < n {
        p <<= 1;
    }
    p
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
    mipmap_levels: usize,
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

    let raw_combined_data: Vec<u8> = mipmaps.into_iter().flatten().collect();

    // 4. 压缩
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

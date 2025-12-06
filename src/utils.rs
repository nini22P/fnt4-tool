pub fn generate_sjis_map() -> Vec<u32> {
    let mut map = Vec::with_capacity(8000);

    for i in 0x20..=0x7F {
        map.push(i);
    }

    for i in 0xA0..=0xDF {
        map.push(i);
    }

    for high in 0x81..=0x9F {
        for low in 0x40..=0xFC {
            if (low & 0x7F) == 0x7F {
                continue;
            }
            map.push((high << 8) | low);
        }
    }

    for high in 0xE0..=0xEE {
        for low in 0x40..=0xFC {
            if (low & 0x7F) == 0x7F {
                continue;
            }
            map.push((high << 8) | low);
        }
    }

    map
}

pub fn decode_sjis_u32(code: u32) -> Option<char> {
    let bytes = if code <= 0xFF {
        vec![code as u8]
    } else {
        vec![(code >> 8) as u8, (code & 0xFF) as u8]
    };

    let (cow, _, had_errors) = encoding_rs::SHIFT_JIS.decode(&bytes);

    if had_errors { None } else { cow.chars().next() }
}

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

pub fn downsample_lanczos(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
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

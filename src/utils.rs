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

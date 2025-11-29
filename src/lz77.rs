// Ported from https://github.com/lzhhzl/about-shin/blob/main/konosuba_py/lz77.py

pub fn decompress(input_data: &[u8], seek_bits: usize, backseek_nbyte: usize) -> Vec<u8> {
    let mut input_pos = 0;
    let mut output = Vec::new();

    while input_pos < input_data.len() {
        let map_byte = input_data[input_pos];
        input_pos += 1;

        for i in 0..8 {
            if input_pos >= input_data.len() {
                break;
            }

            if ((map_byte >> i) & 1) == 0 {
                // Literal byte
                output.push(input_data[input_pos]);
                input_pos += 1;
            } else {
                // Back reference
                let backseek_spec = if backseek_nbyte == 2 {
                    let hi = input_data[input_pos] as u16;
                    let lo = input_data[input_pos + 1] as u16;
                    input_pos += 2;
                    (hi << 8) | lo // Big endian
                } else {
                    let val = input_data[input_pos] as u16;
                    input_pos += 1;
                    val
                };

                let (back_offset, back_length) = if backseek_nbyte == 2 {
                    // FNT4 v1: offset in lower bits, length in upper bits
                    let offset_bits = seek_bits;
                    let back_offset_mask = (1u16 << offset_bits) - 1;
                    let back_length = ((backseek_spec >> offset_bits) + 3) as usize;
                    let back_offset = ((backseek_spec & back_offset_mask) + 1) as usize;
                    (back_offset, back_length)
                } else {
                    // FNT4 v0: length in lower bits, offset in upper bits
                    let len_bits = seek_bits;
                    let back_len_mask = (1u16 << len_bits) - 1;
                    let back_length = ((backseek_spec & back_len_mask) + 2) as usize;
                    let back_offset = ((backseek_spec >> len_bits) + 1) as usize;
                    (back_offset, back_length)
                };

                for _ in 0..back_length {
                    let last = output.len() - back_offset;
                    let byte = output[last];
                    output.push(byte);
                }
            }
        }
    }

    output
}

#[derive(Clone, Debug)]
enum Instruction {
    Literal(u8),
    Reference { length: usize, offset: usize },
}

pub fn compress(input_bytes: &[u8], offset_bits: usize) -> Vec<u8> {
    if input_bytes.is_empty() {
        return Vec::new();
    }

    let count_bits = 16 - offset_bits;
    let max_count = ((1usize << count_bits) - 1) + 3;
    let max_offset = ((1usize << offset_bits) - 1) + 1;

    fn find_offset(search_bytes: &[u8], map_bytes: &[u8]) -> usize {
        for i in 0..search_bytes.len() {
            let pos = search_bytes.len() - i - 1;
            if search_bytes[pos] == map_bytes[0] && search_bytes[pos..].starts_with(map_bytes) {
                return i + 1;
            }
        }
        panic!("find_offset: pattern not found");
    }

    fn all_the_same(input_list: &[u8], compare: u8) -> bool {
        input_list.iter().all(|&item| item == compare)
    }

    let mut instructions: Vec<Instruction> = vec![Instruction::Literal(input_bytes[0])];
    let mut log_len: usize = 1;
    let mut map_bytes: Vec<u8> = Vec::new();
    let mut search_buf: Option<&[u8]> = None;
    let mut len_offset: Option<(usize, usize)> = None;

    let mut i: usize = 1;
    while i < input_bytes.len() {
        let log_bytes = &input_bytes[..log_len];

        if !map_bytes.is_empty() {
            let search_buf_ref = search_buf.unwrap();
            let len_offset_ref = len_offset.unwrap();

            if len_offset_ref.0 == len_offset_ref.1 && input_bytes[i] == map_bytes[0] {
                let main_map_len = map_bytes.len();
                let mut sub_map_len = main_map_len;
                let mut sub_pos = i;

                while (max_count - map_bytes.len()) > 0 {
                    if (max_count - map_bytes.len()) < main_map_len {
                        sub_map_len = max_count - map_bytes.len();
                    }
                    if sub_pos + sub_map_len > input_bytes.len()
                        || &input_bytes[sub_pos..sub_pos + sub_map_len] != &map_bytes[..sub_map_len]
                    {
                        break;
                    }
                    map_bytes.extend_from_slice(&map_bytes[..sub_map_len].to_vec());
                    sub_pos += sub_map_len;
                }

                if map_bytes.len() < max_count {
                    for j in (1..=map_bytes.len()).rev() {
                        if sub_pos + j <= input_bytes.len()
                            && &input_bytes[sub_pos..sub_pos + j] == &map_bytes[..j]
                        {
                            let part = map_bytes[..j].to_vec();
                            map_bytes.extend_from_slice(&part);
                            sub_pos += j;
                            break;
                        }
                    }
                }

                i = sub_pos;
                len_offset = Some((map_bytes.len(), len_offset_ref.1));
                let len_offset_ref = len_offset.unwrap();

                if len_offset_ref.0 == max_count || i == input_bytes.len() {
                    if map_bytes.len() > 0 && map_bytes.len() < 3 {
                        if len_offset_ref.0 == 2 {
                            if all_the_same(&map_bytes, map_bytes[0]) && len_offset_ref.1 == 1 {
                                for &b in &map_bytes {
                                    instructions.push(Instruction::Literal(b));
                                }
                            }
                        } else {
                            panic!("usually will not run in here, please debug");
                        }
                    } else {
                        instructions.push(Instruction::Reference {
                            length: len_offset_ref.0,
                            offset: len_offset_ref.1,
                        });
                    }
                    log_len += map_bytes.len();
                    map_bytes.clear();
                    search_buf = None;
                    len_offset = None;
                    continue;
                }
            }

            let mut test_bytes = map_bytes.clone();
            test_bytes.push(input_bytes[i]);

            if !contains_slice(search_buf_ref, &test_bytes) {
                if map_bytes.len() > 0 && map_bytes.len() < 3 {
                    if map_bytes.len() == 2
                        && (!all_the_same(&map_bytes, map_bytes[0])
                            || contains_slice(search_buf_ref, &[map_bytes[1], input_bytes[i]]))
                    {
                        map_bytes.truncate(1);
                        i -= 1;
                    }
                    for &b in &map_bytes {
                        instructions.push(Instruction::Literal(b));
                    }
                } else {
                    let len_offset_val = (map_bytes.len(), len_offset_ref.1);
                    if len_offset_val.0 == 2 {
                        panic!("usually will not run in here, please debug");
                    }
                    instructions.push(Instruction::Reference {
                        length: len_offset_val.0,
                        offset: len_offset_val.1,
                    });
                }
                log_len += map_bytes.len();
                map_bytes.clear();
                search_buf = None;
                len_offset = None;
            } else {
                if map_bytes.len() == max_count {
                    let offset = find_offset(search_buf_ref, &map_bytes);
                    instructions.push(Instruction::Reference {
                        length: map_bytes.len(),
                        offset,
                    });
                    log_len += map_bytes.len();
                    map_bytes.clear();
                    search_buf = None;
                } else {
                    map_bytes.push(input_bytes[i]);
                    let offset = find_offset(search_buf_ref, &map_bytes);
                    len_offset = Some((map_bytes.len(), offset));

                    if i + 1 == input_bytes.len() {
                        let len_offset_ref = len_offset.unwrap();
                        if len_offset_ref.0 < 3 {
                            for &b in &map_bytes {
                                instructions.push(Instruction::Literal(b));
                            }
                        } else {
                            instructions.push(Instruction::Reference {
                                length: len_offset_ref.0,
                                offset: len_offset_ref.1,
                            });
                        }
                        log_len += map_bytes.len();
                    }
                    i += 1;
                }
            }
        } else {
            if search_buf.is_none() {
                let start = if log_bytes.len() > 1023 {
                    log_bytes.len() - max_offset
                } else {
                    0
                };
                search_buf = Some(&input_bytes[start..log_len]);
            }

            let search_buf_ref = search_buf.unwrap();

            if contains_slice(search_buf_ref, &[input_bytes[i]]) && i + 1 != input_bytes.len() {
                map_bytes.push(input_bytes[i]);
                let offset = find_offset(search_buf_ref, &map_bytes);
                len_offset = Some((1, offset));
            } else {
                instructions.push(Instruction::Literal(input_bytes[i]));
                log_len += 1;
                search_buf = None;
            }
            i += 1;
        }
    }

    encode_instructions(&instructions, offset_bits, max_count, max_offset)
}

fn contains_slice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }
    let first = needle[0];
    let needle_len = needle.len();
    let mut pos = 0;
    while pos + needle_len <= haystack.len() {
        if let Some(idx) = haystack[pos..].iter().position(|&b| b == first) {
            let start = pos + idx;
            if start + needle_len <= haystack.len()
                && &haystack[start..start + needle_len] == needle
            {
                return true;
            }
            pos = start + 1;
        } else {
            break;
        }
    }
    false
}

fn encode_instructions(
    instructions: &[Instruction],
    offset_bits: usize,
    max_count: usize,
    max_offset: usize,
) -> Vec<u8> {
    let count_bits = 16 - offset_bits;
    let mut result = Vec::new();

    for chunk in instructions.chunks(8) {
        let bitmap: u8 = chunk
            .iter()
            .enumerate()
            .map(|(i, instr)| match instr {
                Instruction::Reference { .. } => 1u8 << i,
                Instruction::Literal(_) => 0,
            })
            .sum();

        result.push(bitmap);

        for instr in chunk {
            match instr {
                Instruction::Reference { length, offset } => {
                    assert!(*length <= max_count && *length >= 3);
                    assert!(*offset <= max_offset && *offset > 0);
                    let len_b = (((length - 3) << (8 - count_bits)) | ((offset - 1) >> 8)) as u8;
                    let offset_b = ((offset - 1) & 0xff) as u8;
                    result.push(len_b);
                    result.push(offset_b);
                }
                Instruction::Literal(byte) => {
                    result.push(*byte);
                }
            }
        }
    }

    result
}

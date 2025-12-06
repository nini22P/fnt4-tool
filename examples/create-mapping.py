import csv
import os
import toml
import json

def is_valid_sjis_slot(char):
    """
    检查字符是否符合 Shift-JIS 双字节编码的基本要求
    (这通常是日文游戏字体进行字符劫持所必需的)。
    """
    try:
        b = char.encode('shift_jis', errors='strict')
    except:
        return False
        
    # 必须是双字节字符 (例如，汉字、全角假名)
    if len(b) != 2:
        return False
        
    lead = b[0]
    trail = b[1]
    
    # 尾字节 >= 0x80 是双字节 Shift-JIS 编码的常见特性
    if trail < 0x80:
        return False
        
    return True

def is_cjk_ideograph(char):
    """
    检查一个字符是否属于主要的 CJK 统一表意文字 (汉字) 块。
    """
    code_int = ord(char)
    # CJK 统一表意文字块 (U+4E00–U+9FFF)
    return 0x4E00 <= code_int <= 0x9FFF

def main():
    csv_path = 'main.csv'           # shin-tl 提取的 CSV 文件
    original_col = 's'              # 原始日文
    translated_col = 'translated'   # 翻译后的文本
    metadata_path = 'metadata.toml' # fnt4-tool 提取的 metadata
    
    mapping_output = 'mapping.toml' # 生成的映射表，给 fnt4-tool 使用
    csv_output = 'main_mapped.csv'  # 生成的映射后的 CSV 文件，给 shin-tl 使用

    with open(metadata_path, 'r', encoding='utf-8') as f:
        meta_data = toml.load(f)
    
    font_inventory = {}
    glyphs_section = meta_data.get('glyphs', {})
    iterator = glyphs_section.values() if isinstance(glyphs_section, dict) else glyphs_section
    
    for g in iterator:
        if g.get('char_code'):
            code = int(g['char_code'], 16)
            font_inventory[chr(code)] = code

    needed_chars = set()
    survivor_chars = set()
    
    rows = []
    with open(csv_path, 'r', encoding='utf-8', newline='') as f:
        reader = csv.DictReader(f)
        fieldnames = reader.fieldnames
        for row in reader:
            t_text = row.get(translated_col, '')
            if t_text:
                for c in t_text:
                    if ord(c) >= 0x80: needed_chars.add(c)
            s_text = row.get(original_col, '')
            if s_text:
                for c in s_text:
                    survivor_chars.add(c)
            rows.append(row)

    available_slots = []
    seen_slots = set()

    candidates = sorted(list(survivor_chars), key=lambda x: font_inventory.get(x, 0)) + \
                 sorted(font_inventory.keys(), key=lambda k: font_inventory[k])

    for char in candidates:
        if char not in font_inventory: continue
        if char in seen_slots: continue
        if char in needed_chars: continue
        
        if not is_cjk_ideograph(char):
            continue
            
        code_int = font_inventory[char]
        if code_int < 0x80: continue
        
        if not is_valid_sjis_slot(char): 
            continue

        available_slots.append((code_int, char))
        seen_slots.add(char)

    missing_chars = sorted(list(needed_chars - set(font_inventory.keys())))
    
    if len(missing_chars) > len(available_slots):
        print(f"❌ 警告: 槽位不足! 缺口: {len(missing_chars) - len(available_slots)}")
        missing_chars = missing_chars[:len(available_slots)]

    final_mapping = {}
    trans_table = {}
    
    for i, cn_char in enumerate(missing_chars):
        _, slot_jp_char = available_slots[i]
        final_mapping[slot_jp_char] = cn_char
        trans_table[ord(cn_char)] = slot_jp_char

    with open(mapping_output, 'w', encoding='utf-8') as f:
        f.write("# 替换映射表\n[replace]\n")
        for k, v in final_mapping.items():
            k_s = json.dumps(k, ensure_ascii=False)
            v_s = json.dumps(v, ensure_ascii=False)
            f.write(f"{k_s} = {v_s}\n")

    with open(csv_output, 'w', encoding='utf-8', newline='') as f:
        writer = csv.DictWriter(f, fieldnames=fieldnames)
        writer.writeheader()
        for row in rows:
            orig = row.get(translated_col, '')
            if orig:
                row[translated_col] = orig.translate(trans_table)
            writer.writerow(row)

    print("映射表和 CSV 文件生成完成。")

if __name__ == '__main__':
    main()
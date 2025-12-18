import csv
import os
import toml
import json

def is_valid_sjis_slot(char):
    """
    检查字符是否符合 Shift-JIS 双字节编码的基本要求
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
    return 0x4E00 <= code_int <= 0x9FFF

def main():
    csv_path = 'main.csv'           # 输入 CSV
    original_col = 's'              # 原始日文列
    translated_col = 'translated'   # 翻译列
    metadata_path = 'metadata.toml' # 字体元数据
    
    mapping_output = 'mapping.toml' # 输出映射表
    csv_output = 'main_mapped.csv'  # 输出映射后的 CSV

    if not os.path.exists(metadata_path):
        print(f"错误: 找不到 {metadata_path}")
        return

    with open(metadata_path, 'r', encoding='utf-8') as f:
        meta_data = toml.load(f)
    
    version = meta_data.get('version', 'v0').lower()
    print(f"FNT4 Version: {version}")

    font_inventory = {}
    glyphs_section = meta_data.get('glyphs', {})
    iterator = glyphs_section.values() if isinstance(glyphs_section, dict) else glyphs_section
    
    for g in iterator:
        if g.get('char_code'):
            raw_code = int(g['char_code'], 16)
            try:
                if version == 'v1':
                    char_obj = chr(raw_code)
                else:
                    if raw_code <= 0xFF:
                        # 单字节 (半角)
                        char_obj = raw_code.to_bytes(1, 'big').decode('shift_jis')
                    else:
                        # 双字节 (全角/汉字)
                        char_obj = raw_code.to_bytes(2, 'big').decode('shift_jis')
                
                if char_obj:
                    font_inventory[char_obj] = raw_code
            except:
                continue

    needed_chars = set()     # 译文中需要显示，但字体库里没有的汉字
    chars_in_csv = set()     # CSV 原文 + 译文中出现过的所有字符
    
    rows = []
    if not os.path.exists(csv_path):
        print(f"错误: 找不到 {csv_path}")
        return

    with open(csv_path, 'r', encoding='utf-8', newline='') as f:
        reader = csv.DictReader(f)
        fieldnames = reader.fieldnames
        for row in reader:
            t_text = row.get(translated_col, '')
            s_text = row.get(original_col, '')
            
            if t_text:
                for c in t_text:
                    chars_in_csv.add(c)
                    # 如果译文中的字符不在原字体库中，则标记为“需要映射”
                    if ord(c) >= 0x80 and c not in font_inventory:
                        needed_chars.add(c)
            
            if s_text:
                for c in s_text:
                    chars_in_csv.add(c)
            
            rows.append(row)

    potential_slots = [
        c for c in font_inventory.keys()
        if is_cjk_ideograph(c) and is_valid_sjis_slot(c) and c not in needed_chars
    ]

    # 优先级 1: 在 CSV (原文+译文) 中完全没出现过的字符
    # 优先级 2: 在 CSV 原文中出现了，但在译文中没用到的字符
    unused_slots = [c for c in potential_slots if c not in chars_in_csv]
    low_priority_slots = [c for c in potential_slots if c in chars_in_csv]

    unused_slots.sort(key=lambda x: font_inventory[x])
    low_priority_slots.sort(key=lambda x: font_inventory[x])

    final_candidates = unused_slots + low_priority_slots
    missing_chars = sorted(list(needed_chars))

    print(f"需要映射的汉字数量: {len(missing_chars)}")
    print(f"完全空闲的槽位数量: {len(unused_slots)}")
    print(f"备用(原文冲突)槽位数量: {len(low_priority_slots)}")

    if len(missing_chars) > len(final_candidates):
        print(f"⚠️ 警告: 槽位严重不足! 缺口: {len(missing_chars) - len(final_candidates)}")
        missing_chars = missing_chars[:len(final_candidates)]

    final_mapping = {}  # 原日文字符 -> 新中文字符 (用于 mapping.toml)
    trans_table = {}    # 中文字符 Unicode -> 原日文字符 (用于 CSV 替换)
    
    for i, cn_char in enumerate(missing_chars):
        slot_jp_char = final_candidates[i]
        final_mapping[slot_jp_char] = cn_char
        trans_table[ord(cn_char)] = slot_jp_char

    with open(mapping_output, 'w', encoding='utf-8') as f:
        f.write("# Generated Mapping Table for fnt4-tool\n[replace]\n")
        for jp_char, cn_char in final_mapping.items():
            k_s = json.dumps(jp_char, ensure_ascii=False)
            v_s = json.dumps(cn_char, ensure_ascii=False)
            f.write(f"{k_s} = {v_s}\n")

    with open(csv_output, 'w', encoding='utf-8', newline='') as f:
        writer = csv.DictWriter(f, fieldnames=fieldnames)
        writer.writeheader()
        for row in rows:
            orig = row.get(translated_col, '')
            if orig:
                row[translated_col] = orig.translate(trans_table)
            writer.writerow(row)

    print(f"完成！映射表已写入 {mapping_output}，处理后的 CSV 已写入 {csv_output}。")

if __name__ == '__main__':
    main()
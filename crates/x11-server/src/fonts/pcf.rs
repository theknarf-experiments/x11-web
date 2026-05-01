use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use super::types::{BitmapFont, CharInfo, GlyphBitmap};

const PCF_MAGIC: u32 = 0x70636601; // "\1fcp" in LE

// PCF table types
const PCF_PROPERTIES: u32 = 1 << 0;
const PCF_ACCELERATORS: u32 = 1 << 1;
const PCF_METRICS: u32 = 1 << 2;
const PCF_BITMAPS: u32 = 1 << 3;
const PCF_BDF_ENCODINGS: u32 = 1 << 5;
const PCF_BDF_ACCELERATORS: u32 = 1 << 8;

// Format flags
const PCF_ACCEL_W_INKBOUNDS: u32 = 0x00000100;
const PCF_COMPRESSED_METRICS: u32 = 0x00000100;
const PCF_BYTE_MASK: u32 = 1 << 2; // MSB byte order
const PCF_BIT_MASK: u32 = 1 << 3; // MSB bit order
const PCF_GLYPH_PAD_MASK: u32 = 3; // 2 bits for glyph padding

fn pcf_read_u32(data: &[u8], offset: usize, msb: bool) -> u32 {
    if offset + 4 > data.len() {
        return 0;
    }
    if msb {
        u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ])
    } else {
        u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ])
    }
}

fn pcf_read_u16(data: &[u8], offset: usize, msb: bool) -> u16 {
    if offset + 2 > data.len() {
        return 0;
    }
    if msb {
        u16::from_be_bytes([data[offset], data[offset + 1]])
    } else {
        u16::from_le_bytes([data[offset], data[offset + 1]])
    }
}

fn pcf_read_i16(data: &[u8], offset: usize, msb: bool) -> i16 {
    pcf_read_u16(data, offset, msb) as i16
}

fn pcf_read_i32(data: &[u8], offset: usize, msb: bool) -> i32 {
    pcf_read_u32(data, offset, msb) as i32
}

struct PcfTable {
    table_type: u32,
    format: u32,
    size: u32,
    offset: u32,
}

pub(super) fn load_pcf_font(path: &Path) -> Option<BitmapFont> {
    let data = std::fs::read(path).ok()?;
    parse_pcf_data(&data, path)
}

pub(super) fn load_pcf_gz_font(path: &Path) -> Option<BitmapFont> {
    let file = std::fs::File::open(path).ok()?;
    let mut decoder = flate2::read::GzDecoder::new(file);
    let mut data = Vec::new();
    decoder.read_to_end(&mut data).ok()?;
    parse_pcf_data(&data, path)
}

pub(super) fn parse_pcf_data(data: &[u8], path: &Path) -> Option<BitmapFont> {
    if data.len() < 8 {
        return None;
    }

    // Check magic (always LE)
    let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if magic != PCF_MAGIC {
        return None;
    }

    let table_count = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    if data.len() < 8 + table_count * 16 {
        return None;
    }

    let mut tables = Vec::with_capacity(table_count);
    for i in 0..table_count {
        let off = 8 + i * 16;
        tables.push(PcfTable {
            table_type: u32::from_le_bytes([
                data[off],
                data[off + 1],
                data[off + 2],
                data[off + 3],
            ]),
            format: u32::from_le_bytes([
                data[off + 4],
                data[off + 5],
                data[off + 6],
                data[off + 7],
            ]),
            size: u32::from_le_bytes([
                data[off + 8],
                data[off + 9],
                data[off + 10],
                data[off + 11],
            ]),
            offset: u32::from_le_bytes([
                data[off + 12],
                data[off + 13],
                data[off + 14],
                data[off + 15],
            ]),
        });
    }

    let find_table = |tt: u32| -> Option<&PcfTable> { tables.iter().find(|t| t.table_type == tt) };

    // Parse metrics
    let metrics_table = find_table(PCF_METRICS)?;
    let metrics = parse_pcf_metrics(data, metrics_table)?;

    // Parse bitmaps
    let bitmaps_table = find_table(PCF_BITMAPS)?;
    let bitmaps = parse_pcf_bitmaps(data, bitmaps_table, metrics.len(), &metrics)?;

    // Parse encodings
    let encodings_table = find_table(PCF_BDF_ENCODINGS)?;
    let (min_char, max_char, encoding_map) = parse_pcf_encodings(data, encodings_table)?;

    // Parse properties for font name
    let font_name = if let Some(props_table) = find_table(PCF_PROPERTIES) {
        parse_pcf_properties_font_name(data, props_table)
    } else {
        None
    };

    // Parse accelerators for ascent/descent
    let accel_table = find_table(PCF_BDF_ACCELERATORS).or_else(|| find_table(PCF_ACCELERATORS));
    let (font_ascent, font_descent) = if let Some(at) = accel_table {
        parse_pcf_accelerators(data, at)
    } else {
        // Derive from metrics
        let mut max_asc = 0i16;
        let mut max_desc = 0i16;
        for m in &metrics {
            max_asc = max_asc.max(m.ascent);
            max_desc = max_desc.max(m.descent);
        }
        (max_asc, max_desc)
    };

    // Build the BitmapFont
    let num_chars = if max_char >= min_char {
        (max_char - min_char + 1) as usize
    } else {
        0
    };
    let mut char_infos = vec![CharInfo::default(); num_chars];
    let mut glyphs_vec = vec![
        GlyphBitmap {
            width: 0,
            height: 0,
            bitmap: Vec::new(),
        };
        num_chars
    ];

    let mut min_bounds = CharInfo {
        left_side_bearing: i16::MAX,
        right_side_bearing: i16::MAX,
        character_width: i16::MAX,
        ascent: i16::MAX,
        descent: i16::MAX,
        attributes: 0,
    };
    let mut max_bounds = CharInfo::default();

    for encoding in min_char..=max_char {
        let idx = (encoding - min_char) as usize;
        let glyph_idx = match encoding_map.get(&encoding) {
            Some(&gi) => gi,
            None => continue,
        };

        if glyph_idx >= metrics.len() {
            continue;
        }

        let m = &metrics[glyph_idx];
        let ci = CharInfo {
            left_side_bearing: m.left_side_bearing,
            right_side_bearing: m.right_side_bearing,
            character_width: m.character_width,
            ascent: m.ascent,
            descent: m.descent,
            attributes: m.attributes,
        };

        min_bounds.left_side_bearing = min_bounds.left_side_bearing.min(ci.left_side_bearing);
        min_bounds.right_side_bearing = min_bounds.right_side_bearing.min(ci.right_side_bearing);
        min_bounds.character_width = min_bounds.character_width.min(ci.character_width);
        min_bounds.ascent = min_bounds.ascent.min(ci.ascent);
        min_bounds.descent = min_bounds.descent.min(ci.descent);

        max_bounds.left_side_bearing = max_bounds.left_side_bearing.max(ci.left_side_bearing);
        max_bounds.right_side_bearing = max_bounds.right_side_bearing.max(ci.right_side_bearing);
        max_bounds.character_width = max_bounds.character_width.max(ci.character_width);
        max_bounds.ascent = max_bounds.ascent.max(ci.ascent);
        max_bounds.descent = max_bounds.descent.max(ci.descent);

        if idx < char_infos.len() {
            char_infos[idx] = ci;
        }

        // Get bitmap for this glyph
        if glyph_idx < bitmaps.len() && idx < glyphs_vec.len() {
            let w = (m.right_side_bearing - m.left_side_bearing).max(0) as u16;
            let h = (m.ascent + m.descent).max(0) as u16;
            glyphs_vec[idx] = GlyphBitmap {
                width: w,
                height: h,
                bitmap: bitmaps[glyph_idx].clone(),
            };
        }
    }

    let name = font_name.unwrap_or_else(|| {
        path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    });

    Some(BitmapFont {
        name,
        min_bounds,
        max_bounds,
        min_char,
        max_char,
        default_char: 32,
        font_ascent,
        font_descent,
        char_infos,
        glyphs: glyphs_vec,
        scalable_path: None,
        scalable_pixel_size: 0,
    })
}

struct PcfMetric {
    left_side_bearing: i16,
    right_side_bearing: i16,
    character_width: i16,
    ascent: i16,
    descent: i16,
    attributes: u16,
}

fn parse_pcf_metrics(data: &[u8], table: &PcfTable) -> Option<Vec<PcfMetric>> {
    let off = table.offset as usize;
    if off + 4 > data.len() {
        return None;
    }
    let format = pcf_read_u32(data, off, false); // format is always LE
    let msb = format & PCF_BYTE_MASK != 0;
    let compressed = format & PCF_COMPRESSED_METRICS != 0;

    let mut pos = off + 4;
    let mut metrics = Vec::new();

    if compressed {
        // Compressed: 2-byte count, then 5 bytes per metric
        let count = pcf_read_u16(data, pos, msb) as usize;
        pos += 2;
        for _ in 0..count {
            if pos + 5 > data.len() {
                break;
            }
            metrics.push(PcfMetric {
                left_side_bearing: data[pos] as i16 - 0x80,
                right_side_bearing: data[pos + 1] as i16 - 0x80,
                character_width: data[pos + 2] as i16 - 0x80,
                ascent: data[pos + 3] as i16 - 0x80,
                descent: data[pos + 4] as i16 - 0x80,
                attributes: 0,
            });
            pos += 5;
        }
    } else {
        // Uncompressed: 4-byte count, then 12 bytes per metric
        let count = pcf_read_u32(data, pos, msb) as usize;
        pos += 4;
        for _ in 0..count {
            if pos + 12 > data.len() {
                break;
            }
            metrics.push(PcfMetric {
                left_side_bearing: pcf_read_i16(data, pos, msb),
                right_side_bearing: pcf_read_i16(data, pos + 2, msb),
                character_width: pcf_read_i16(data, pos + 4, msb),
                ascent: pcf_read_i16(data, pos + 6, msb),
                descent: pcf_read_i16(data, pos + 8, msb),
                attributes: pcf_read_u16(data, pos + 10, msb),
            });
            pos += 12;
        }
    }

    Some(metrics)
}

fn parse_pcf_bitmaps(
    data: &[u8],
    table: &PcfTable,
    glyph_count: usize,
    metrics: &[PcfMetric],
) -> Option<Vec<Vec<u8>>> {
    let off = table.offset as usize;
    if off + 4 > data.len() {
        return None;
    }
    let format = pcf_read_u32(data, off, false);
    let msb = format & PCF_BYTE_MASK != 0;
    let msb_bits = format & PCF_BIT_MASK != 0;
    let glyph_pad = 1usize << (format & PCF_GLYPH_PAD_MASK);

    let mut pos = off + 4;

    // Glyph count
    let count = pcf_read_u32(data, pos, msb) as usize;
    pos += 4;

    if count != glyph_count || count > 100_000 {
        // Mismatch - try to proceed anyway
    }

    // Offsets into bitmap data (one per glyph)
    let mut offsets = Vec::with_capacity(count);
    for _ in 0..count {
        offsets.push(pcf_read_u32(data, pos, msb) as usize);
        pos += 4;
    }

    // 4 bitmap sizes (for different padding)
    let _sizes: [u32; 4] = [
        pcf_read_u32(data, pos, msb),
        pcf_read_u32(data, pos + 4, msb),
        pcf_read_u32(data, pos + 8, msb),
        pcf_read_u32(data, pos + 12, msb),
    ];
    pos += 16;

    let bitmap_data_start = pos;

    // Extract bitmaps, repacking from PCF's native row padding to 1-byte padding.
    // PCF stores each row padded to `glyph_pad` bytes (1, 2, or 4).
    // Our internal format uses 1-byte padding (row_bytes = ceil(width/8)).
    let mut bitmaps = Vec::with_capacity(count);
    for i in 0..count {
        let bm_off = bitmap_data_start + offsets[i];

        if i >= metrics.len() {
            bitmaps.push(Vec::new());
            continue;
        }

        let m = &metrics[i];
        let w = (m.right_side_bearing - m.left_side_bearing).max(0) as usize;
        let h = (m.ascent + m.descent).max(0) as usize;

        if w == 0 || h == 0 {
            bitmaps.push(Vec::new());
            continue;
        }

        // Row stride in the PCF file: ceil(ceil(w/8) / glyph_pad) * glyph_pad
        let pcf_row_bytes = (w.div_ceil(8) + glyph_pad - 1) / glyph_pad * glyph_pad;
        // Our internal row stride: ceil(w/8)
        let dst_row_bytes = w.div_ceil(8);

        let mut bitmap = vec![0u8; dst_row_bytes * h];
        for row in 0..h {
            let src_start = bm_off + row * pcf_row_bytes;
            let dst_start = row * dst_row_bytes;
            for b in 0..dst_row_bytes {
                let src_idx = src_start + b;
                if src_idx < data.len() {
                    let mut byte = data[src_idx];
                    // If bit order is LSB-first, reverse bits
                    if !msb_bits {
                        byte = byte.reverse_bits();
                    }
                    bitmap[dst_start + b] = byte;
                }
            }
        }

        bitmaps.push(bitmap);
    }

    Some(bitmaps)
}

fn parse_pcf_encodings(data: &[u8], table: &PcfTable) -> Option<(u16, u16, HashMap<u16, usize>)> {
    let off = table.offset as usize;
    if off + 14 > data.len() {
        return None;
    }
    let format = pcf_read_u32(data, off, false);
    let msb = format & PCF_BYTE_MASK != 0;

    let min_byte2 = pcf_read_u16(data, off + 4, msb);
    let max_byte2 = pcf_read_u16(data, off + 6, msb);
    let min_byte1 = pcf_read_u16(data, off + 8, msb);
    let max_byte1 = pcf_read_u16(data, off + 10, msb);
    let _default_char = pcf_read_u16(data, off + 12, msb);

    let mut pos = off + 14;
    let mut encoding_map = HashMap::new();

    // For single-byte fonts, min_byte1 == max_byte1 == 0
    for b1 in min_byte1..=max_byte1 {
        for b2 in min_byte2..=max_byte2 {
            if pos + 2 > data.len() {
                break;
            }
            let glyph_idx = pcf_read_u16(data, pos, msb);
            pos += 2;
            if glyph_idx != 0xFFFF {
                let encoding = if min_byte1 == 0 && max_byte1 == 0 {
                    b2
                } else {
                    (b1 << 8) | b2
                };
                encoding_map.insert(encoding, glyph_idx as usize);
            }
        }
    }

    let min_char = min_byte2;
    let max_char = if min_byte1 == 0 && max_byte1 == 0 {
        max_byte2
    } else {
        (max_byte1 << 8) | max_byte2
    };

    Some((min_char, max_char, encoding_map))
}

fn parse_pcf_properties_font_name(data: &[u8], table: &PcfTable) -> Option<String> {
    let off = table.offset as usize;
    if off + 8 > data.len() {
        return None;
    }
    let format = pcf_read_u32(data, off, false);
    let msb = format & PCF_BYTE_MASK != 0;

    let num_props = pcf_read_u32(data, off + 4, msb) as usize;
    if num_props > 10_000 {
        return None;
    }

    let props_start = off + 8;
    // Each property: name_offset(4), is_string(1), value(4) = 9 bytes
    let strings_start = props_start + num_props * 9;
    // Align to 4 bytes
    let strings_start = (strings_start + 3) & !3;

    if strings_start + 4 > data.len() {
        return None;
    }
    let string_size = pcf_read_u32(data, strings_start, msb) as usize;
    let string_data_start = strings_start + 4;

    if string_data_start + string_size > data.len() {
        return None;
    }

    let strings = &data[string_data_start..string_data_start + string_size];

    // Look for FONT property
    for i in 0..num_props {
        let poff = props_start + i * 9;
        if poff + 9 > data.len() {
            break;
        }
        let name_offset = pcf_read_u32(data, poff, msb) as usize;
        let is_string = data[poff + 4];
        let value = pcf_read_u32(data, poff + 5, msb);

        if name_offset < strings.len() {
            let name_end = strings[name_offset..]
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(strings.len() - name_offset);
            let name = std::str::from_utf8(&strings[name_offset..name_offset + name_end]).ok()?;

            if name == "FONT" && is_string != 0 {
                let val_offset = value as usize;
                if val_offset < strings.len() {
                    let val_end = strings[val_offset..]
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(strings.len() - val_offset);
                    return std::str::from_utf8(&strings[val_offset..val_offset + val_end])
                        .ok()
                        .map(|s| s.to_string());
                }
            }
        }
    }

    None
}

fn parse_pcf_accelerators(data: &[u8], table: &PcfTable) -> (i16, i16) {
    let off = table.offset as usize;
    if off + 4 > data.len() {
        return (10, 3);
    }
    let format = pcf_read_u32(data, off, false);
    let msb = format & PCF_BYTE_MASK != 0;
    let _has_ink = format & PCF_ACCEL_W_INKBOUNDS != 0;

    // Layout: format(4), noOverlap(1), constantMetrics(1),
    //         terminalFont(1), constantWidth(1), inkInside(1),
    //         inkMetrics(1), drawDirection(1), padding(1),
    //         fontAscent(4), fontDescent(4), maxOverlap(4),
    //         then minbounds(12) and maxbounds(12)
    //         then optionally ink_minbounds(12) and ink_maxbounds(12)

    let ascent_off = off + 12;
    let descent_off = off + 16;

    if descent_off + 4 > data.len() {
        return (10, 3);
    }

    let font_ascent = pcf_read_i32(data, ascent_off, msb) as i16;
    let font_descent = pcf_read_i32(data, descent_off, msb) as i16;

    (font_ascent, font_descent)
}

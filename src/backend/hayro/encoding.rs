pub(super) fn decode_pdf_text_string(bytes: &[u8]) -> String {
    if let Some(decoded) = decode_bom_prefixed_text(bytes) {
        return decoded;
    }

    bytes
        .iter()
        .map(|byte| decode_pdf_doc_encoding_byte(*byte))
        .collect()
}

fn decode_bom_prefixed_text(bytes: &[u8]) -> Option<String> {
    match bytes {
        [0xFE, 0xFF, rest @ ..] => decode_utf16_bytes(rest, Utf16Endian::Big),
        [0xFF, 0xFE, rest @ ..] => decode_utf16_bytes(rest, Utf16Endian::Little),
        [0xEF, 0xBB, 0xBF, rest @ ..] => Some(String::from_utf8_lossy(rest).into_owned()),
        _ => None,
    }
}

fn decode_utf16_bytes(bytes: &[u8], endian: Utf16Endian) -> Option<String> {
    let chunks = bytes.chunks_exact(2);
    if !chunks.remainder().is_empty() {
        return None;
    }

    let code_units = chunks
        .map(|chunk| match endian {
            Utf16Endian::Big => u16::from_be_bytes([chunk[0], chunk[1]]),
            Utf16Endian::Little => u16::from_le_bytes([chunk[0], chunk[1]]),
        })
        .collect::<Vec<_>>();

    Some(String::from_utf16_lossy(&code_units))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Utf16Endian {
    Big,
    Little,
}
fn decode_pdf_doc_encoding_byte(byte: u8) -> char {
    match byte {
        0x16 => '\u{0016}',
        0x18 => '\u{02D8}',
        0x19 => '\u{02C7}',
        0x1A => '\u{02C6}',
        0x1B => '\u{02D9}',
        0x1C => '\u{02DD}',
        0x1D => '\u{02DB}',
        0x1E => '\u{02DA}',
        0x1F => '\u{02DC}',
        0x7F => '\u{FFFD}',
        0x80 => '\u{2022}',
        0x81 => '\u{2020}',
        0x82 => '\u{2021}',
        0x83 => '\u{2026}',
        0x84 => '\u{2014}',
        0x85 => '\u{2013}',
        0x86 => '\u{0192}',
        0x87 => '\u{2044}',
        0x88 => '\u{2039}',
        0x89 => '\u{203A}',
        0x8A => '\u{2212}',
        0x8B => '\u{2030}',
        0x8C => '\u{201E}',
        0x8D => '\u{201C}',
        0x8E => '\u{201D}',
        0x8F => '\u{2018}',
        0x90 => '\u{2019}',
        0x91 => '\u{201A}',
        0x92 => '\u{2122}',
        0x93 => '\u{FB01}',
        0x94 => '\u{FB02}',
        0x95 => '\u{0141}',
        0x96 => '\u{0152}',
        0x97 => '\u{0160}',
        0x98 => '\u{0178}',
        0x99 => '\u{017D}',
        0x9A => '\u{0131}',
        0x9B => '\u{0142}',
        0x9C => '\u{0153}',
        0x9D => '\u{0161}',
        0x9E => '\u{017E}',
        _ => byte as char,
    }
}

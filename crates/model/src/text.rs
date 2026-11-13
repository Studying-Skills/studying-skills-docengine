//! UTF-16 lengths and slices of UTF-8 strings (document positions count UTF-16 code units).

pub fn utf16_len(s: &str) -> u32 {
    s.chars().map(|c| c.len_utf16() as u32).sum()
}

/// The part of `s` between the UTF-16 offsets `from` and `to`. An offset inside a surrogate pair is moved to the
/// start of that character; offsets past the end are clamped.
pub fn utf16_slice(s: &str, from: u32, to: u32) -> &str {
    let (mut start, mut end) = (s.len(), s.len());
    let (mut found_start, mut units) = (false, 0u32);
    for (byte, c) in s.char_indices() {
        let next = units + c.len_utf16() as u32;
        if !found_start && from < next {
            start = byte;
            found_start = true;
        }
        if to < next {
            end = byte;
            break;
        }
        units = next;
    }
    if !found_start {
        start = s.len();
    }
    s.get(start..end.max(start)).unwrap_or("")
}

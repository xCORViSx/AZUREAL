//! Platform-specific key mappings
//!
//! Handles macOS quirks where ⌥+letter produces unicode characters instead
//! of setting the ALT modifier bit.

/// macOS ⌥+letter produces unicode chars instead of setting the ALT modifier.
/// This maps those unicode chars back to the original letter so handlers can
/// match `⌥+letter` portably. Returns None if the char isn't an ⌥ mapping.
/// Based on macOS US keyboard layout.
#[inline]
#[allow(dead_code)]
pub fn macos_opt_key(ch: char) -> Option<char> {
    match ch {
        'å' => Some('a'), '∫' => Some('b'), 'ç' => Some('c'), '∂' => Some('d'),
        '´' => Some('e'), 'ƒ' => Some('f'), '©' => Some('g'), '˙' => Some('h'),
        'ˆ' => Some('i'), '∆' => Some('j'), '˚' => Some('k'), '¬' => Some('l'),
        'µ' => Some('m'), '˜' => Some('n'), 'ø' => Some('o'), 'π' => Some('p'),
        'œ' => Some('q'), '®' => Some('r'), 'ß' => Some('s'), '†' => Some('t'),
        '¨' => Some('u'), '√' => Some('v'), '∑' => Some('w'), '≈' => Some('x'),
        '¥' => Some('y'), 'Ω' => Some('z'),
        // ⌥+numbers on US keyboard layout
        '¡' => Some('1'), '™' => Some('2'), '£' => Some('3'), '¢' => Some('4'),
        '∞' => Some('5'), '§' => Some('6'), '¶' => Some('7'), '•' => Some('8'),
        'ª' => Some('9'), 'º' => Some('0'),
        _ => None,
    }
}

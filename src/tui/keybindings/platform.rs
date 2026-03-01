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

#[cfg(test)]
mod tests {
    use super::*;

    // ══════════════════════════════════════════════════════════════════
    //  ⌥+letter mappings (a–z)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn opt_a() { assert_eq!(macos_opt_key('å'), Some('a')); }

    #[test]
    fn opt_b() { assert_eq!(macos_opt_key('∫'), Some('b')); }

    #[test]
    fn opt_c() { assert_eq!(macos_opt_key('ç'), Some('c')); }

    #[test]
    fn opt_d() { assert_eq!(macos_opt_key('∂'), Some('d')); }

    #[test]
    fn opt_e() { assert_eq!(macos_opt_key('´'), Some('e')); }

    #[test]
    fn opt_f() { assert_eq!(macos_opt_key('ƒ'), Some('f')); }

    #[test]
    fn opt_g() { assert_eq!(macos_opt_key('©'), Some('g')); }

    #[test]
    fn opt_h() { assert_eq!(macos_opt_key('˙'), Some('h')); }

    #[test]
    fn opt_i() { assert_eq!(macos_opt_key('ˆ'), Some('i')); }

    #[test]
    fn opt_j() { assert_eq!(macos_opt_key('∆'), Some('j')); }

    #[test]
    fn opt_k() { assert_eq!(macos_opt_key('˚'), Some('k')); }

    #[test]
    fn opt_l() { assert_eq!(macos_opt_key('¬'), Some('l')); }

    #[test]
    fn opt_m() { assert_eq!(macos_opt_key('µ'), Some('m')); }

    #[test]
    fn opt_n() { assert_eq!(macos_opt_key('˜'), Some('n')); }

    #[test]
    fn opt_o() { assert_eq!(macos_opt_key('ø'), Some('o')); }

    #[test]
    fn opt_p() { assert_eq!(macos_opt_key('π'), Some('p')); }

    #[test]
    fn opt_q() { assert_eq!(macos_opt_key('œ'), Some('q')); }

    #[test]
    fn opt_r() { assert_eq!(macos_opt_key('®'), Some('r')); }

    #[test]
    fn opt_s() { assert_eq!(macos_opt_key('ß'), Some('s')); }

    #[test]
    fn opt_t() { assert_eq!(macos_opt_key('†'), Some('t')); }

    #[test]
    fn opt_u() { assert_eq!(macos_opt_key('¨'), Some('u')); }

    #[test]
    fn opt_v() { assert_eq!(macos_opt_key('√'), Some('v')); }

    #[test]
    fn opt_w() { assert_eq!(macos_opt_key('∑'), Some('w')); }

    #[test]
    fn opt_x() { assert_eq!(macos_opt_key('≈'), Some('x')); }

    #[test]
    fn opt_y() { assert_eq!(macos_opt_key('¥'), Some('y')); }

    #[test]
    fn opt_z() { assert_eq!(macos_opt_key('Ω'), Some('z')); }

    // ══════════════════════════════════════════════════════════════════
    //  ⌥+number mappings (0–9)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn opt_1() { assert_eq!(macos_opt_key('¡'), Some('1')); }

    #[test]
    fn opt_2() { assert_eq!(macos_opt_key('™'), Some('2')); }

    #[test]
    fn opt_3() { assert_eq!(macos_opt_key('£'), Some('3')); }

    #[test]
    fn opt_4() { assert_eq!(macos_opt_key('¢'), Some('4')); }

    #[test]
    fn opt_5() { assert_eq!(macos_opt_key('∞'), Some('5')); }

    #[test]
    fn opt_6() { assert_eq!(macos_opt_key('§'), Some('6')); }

    #[test]
    fn opt_7() { assert_eq!(macos_opt_key('¶'), Some('7')); }

    #[test]
    fn opt_8() { assert_eq!(macos_opt_key('•'), Some('8')); }

    #[test]
    fn opt_9() { assert_eq!(macos_opt_key('ª'), Some('9')); }

    #[test]
    fn opt_0() { assert_eq!(macos_opt_key('º'), Some('0')); }

    // ══════════════════════════════════════════════════════════════════
    //  ASCII letters return None (not ⌥ mappings)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn ascii_a_none() { assert_eq!(macos_opt_key('a'), None); }

    #[test]
    fn ascii_z_none() { assert_eq!(macos_opt_key('z'), None); }

    #[test]
    fn ascii_upper_a_none() { assert_eq!(macos_opt_key('A'), None); }

    #[test]
    fn ascii_upper_z_none() { assert_eq!(macos_opt_key('Z'), None); }

    #[test]
    fn ascii_m_none() { assert_eq!(macos_opt_key('m'), None); }

    #[test]
    fn ascii_upper_m_none() { assert_eq!(macos_opt_key('M'), None); }

    // ══════════════════════════════════════════════════════════════════
    //  ASCII digits return None
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn digit_0_none() { assert_eq!(macos_opt_key('0'), None); }

    #[test]
    fn digit_1_none() { assert_eq!(macos_opt_key('1'), None); }

    #[test]
    fn digit_5_none() { assert_eq!(macos_opt_key('5'), None); }

    #[test]
    fn digit_9_none() { assert_eq!(macos_opt_key('9'), None); }

    // ══════════════════════════════════════════════════════════════════
    //  Common special characters return None
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn space_none() { assert_eq!(macos_opt_key(' '), None); }

    #[test]
    fn exclamation_none() { assert_eq!(macos_opt_key('!'), None); }

    #[test]
    fn at_sign_none() { assert_eq!(macos_opt_key('@'), None); }

    #[test]
    fn hash_none() { assert_eq!(macos_opt_key('#'), None); }

    #[test]
    fn dollar_none() { assert_eq!(macos_opt_key('$'), None); }

    #[test]
    fn percent_none() { assert_eq!(macos_opt_key('%'), None); }

    #[test]
    fn ampersand_none() { assert_eq!(macos_opt_key('&'), None); }

    #[test]
    fn asterisk_none() { assert_eq!(macos_opt_key('*'), None); }

    #[test]
    fn slash_none() { assert_eq!(macos_opt_key('/'), None); }

    #[test]
    fn backslash_none() { assert_eq!(macos_opt_key('\\'), None); }

    #[test]
    fn tilde_none() { assert_eq!(macos_opt_key('~'), None); }

    #[test]
    fn period_none() { assert_eq!(macos_opt_key('.'), None); }

    #[test]
    fn comma_none() { assert_eq!(macos_opt_key(','), None); }

    // ══════════════════════════════════════════════════════════════════
    //  Control / edge characters return None
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn null_char_none() { assert_eq!(macos_opt_key('\0'), None); }

    #[test]
    fn newline_none() { assert_eq!(macos_opt_key('\n'), None); }

    #[test]
    fn tab_none() { assert_eq!(macos_opt_key('\t'), None); }

    #[test]
    fn carriage_return_none() { assert_eq!(macos_opt_key('\r'), None); }

    #[test]
    fn bell_none() { assert_eq!(macos_opt_key('\x07'), None); }

    #[test]
    fn escape_char_none() { assert_eq!(macos_opt_key('\x1b'), None); }

    // ══════════════════════════════════════════════════════════════════
    //  Extended Unicode that is NOT in the mapping
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn emoji_smiley_none() { assert_eq!(macos_opt_key('😊'), None); }

    #[test]
    fn cjk_char_none() { assert_eq!(macos_opt_key('中'), None); }

    #[test]
    fn cyrillic_char_none() { assert_eq!(macos_opt_key('Д'), None); }

    #[test]
    fn arabic_char_none() { assert_eq!(macos_opt_key('ع'), None); }

    #[test]
    fn greek_alpha_none() {
        // α (lowercase alpha) is not in the mapping — Ω (uppercase omega) IS ⌥+z
        assert_eq!(macos_opt_key('α'), None);
    }

    #[test]
    fn accented_e_none() {
        // è is NOT the same as ´ (⌥+e produces the dead-key acute accent ´)
        assert_eq!(macos_opt_key('è'), None);
    }

    #[test]
    fn yen_is_opt_y() {
        // ¥ is ⌥+y
        assert_eq!(macos_opt_key('¥'), Some('y'));
    }

    #[test]
    fn degree_sign_none() {
        // ° (degree) is ⌥+shift+8, not plain ⌥+key
        assert_eq!(macos_opt_key('°'), None);
    }

    #[test]
    fn en_dash_none() { assert_eq!(macos_opt_key('–'), None); }

    #[test]
    fn em_dash_none() { assert_eq!(macos_opt_key('—'), None); }

    #[test]
    fn copyright_is_opt_g() {
        assert_eq!(macos_opt_key('©'), Some('g'));
    }

    #[test]
    fn registered_is_opt_r() {
        assert_eq!(macos_opt_key('®'), Some('r'));
    }

    #[test]
    fn trademark_is_opt_2() {
        assert_eq!(macos_opt_key('™'), Some('2'));
    }

    #[test]
    fn pilcrow_is_opt_7() {
        assert_eq!(macos_opt_key('¶'), Some('7'));
    }

    #[test]
    fn section_is_opt_6() {
        assert_eq!(macos_opt_key('§'), Some('6'));
    }

    #[test]
    fn bullet_is_opt_8() {
        assert_eq!(macos_opt_key('•'), Some('8'));
    }

    #[test]
    fn infinity_is_opt_5() {
        assert_eq!(macos_opt_key('∞'), Some('5'));
    }

    #[test]
    fn summation_is_opt_w() {
        assert_eq!(macos_opt_key('∑'), Some('w'));
    }

    #[test]
    fn approx_equal_is_opt_x() {
        assert_eq!(macos_opt_key('≈'), Some('x'));
    }

    #[test]
    fn not_sign_is_opt_l() {
        assert_eq!(macos_opt_key('¬'), Some('l'));
    }

    #[test]
    fn micro_is_opt_m() {
        assert_eq!(macos_opt_key('µ'), Some('m'));
    }
}

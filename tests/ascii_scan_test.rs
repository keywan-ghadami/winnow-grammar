//! The word-at-a-time class scan against the definition it stands for.
//!
//! This is the kind of trick that is right for the inputs one thinks of and
//! wrong for one byte in 256, so it is checked exhaustively rather than by
//! example: every class, every byte value, at every offset in a word.

use winnow_grammar::ascii::AsciiClass;

const CLASSES: [(&str, AsciiClass); 8] = [
    ("digit", AsciiClass::DIGIT),
    ("hex", AsciiClass::HEX_DIGIT),
    ("oct", AsciiClass::OCT_DIGIT),
    ("binary", AsciiClass::BINARY_DIGIT),
    ("alpha", AsciiClass::ALPHA),
    ("ident", AsciiClass::IDENT),
    ("space", AsciiClass::SPACE),
    ("multispace", AsciiClass::MULTISPACE),
];

/// What `run` must agree with: the class, byte by byte.
fn by_byte(class: AsciiClass, b: &[u8]) -> usize {
    b.iter().take_while(|&&c| class.contains(c)).count()
}

#[test]
fn the_word_scan_agrees_with_the_byte_definition_everywhere() {
    for (name, class) in CLASSES {
        // Every byte, at every offset of a word and past it: a run of members
        // with one candidate byte planted in it.
        let member = (0u8..=255).find(|&b| class.contains(b)).expect("non-empty");
        for offset in 0..20usize {
            for byte in 0u8..=255 {
                let mut buf = vec![member; 20];
                buf[offset] = byte;
                assert_eq!(
                    class.run(&buf),
                    by_byte(class, &buf),
                    "{name}: byte {byte:#04x} at offset {offset}"
                );
            }
        }
    }
}

#[test]
fn a_short_input_is_scanned_by_the_tail_and_agrees() {
    for (name, class) in CLASSES {
        for len in 0..9usize {
            for byte in 0u8..=255 {
                let buf = vec![byte; len];
                assert_eq!(
                    class.run(&buf),
                    by_byte(class, &buf),
                    "{name}: {len} x {byte:#04x}"
                );
            }
        }
    }
}

#[test]
fn a_class_never_stops_inside_a_character() {
    // The safety argument: a continuation byte has its high bit set and is in
    // no ASCII class, so a scan stops before a multi-byte character, never
    // inside one. Slicing there is therefore always a boundary.
    let s = "abc_123üöä€𝄞xyz";
    for (_, class) in CLASSES {
        let n = class.run(s.as_bytes());
        assert!(s.is_char_boundary(n), "{n} is not a boundary in {s:?}");
    }
}

#[test]
fn the_wide_continuation_decodes_only_where_it_must() {
    // `raw_ident`'s class is Unicode alphanumeric: the ASCII part is scanned
    // by word and a character is built only at a byte that is not ASCII.
    let wide = |c: char| c.is_alphanumeric();
    let cases = [
        ("simple_name ", 11),
        ("über ", 5),     // 'ü' is two bytes, both counted
        ("a_über_b!", 9), // ASCII, wide, ASCII again
        ("東京 ", 6),     // three bytes each
        ("!", 0),
        ("", 0),
        ("naïve-", 6), // stops at '-', which is neither
    ];
    for (input, expect) in cases {
        assert_eq!(
            AsciiClass::IDENT.run_or_wide(input, wide),
            expect,
            "{input:?}"
        );
        assert!(input.is_char_boundary(AsciiClass::IDENT.run_or_wide(input, wide)));
    }
}

#[test]
fn the_wide_continuation_agrees_with_a_character_walk() {
    // The same definition written the obvious way, over a mix of scripts.
    let wide = |c: char| c.is_alphanumeric();
    for input in [
        "abc",
        "über_straße9",
        "東京タワー",
        "a1_ü2_東3!rest",
        "€uro",
        "_",
        "𝄞note",
    ] {
        let expect: usize = input
            .chars()
            .take_while(|&c| c.is_alphanumeric() || c == '_')
            .map(char::len_utf8)
            .sum();
        assert_eq!(
            AsciiClass::IDENT.run_or_wide(input, wide),
            expect,
            "{input:?}"
        );
    }
}

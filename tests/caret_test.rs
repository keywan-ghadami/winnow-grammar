//! The source line under the message, and the caret under the token.
//!
//! A position a reader has to go and look up is half a diagnostic: every
//! message said `at line 3, column 5` and no message showed line 3. These pin
//! the cases where getting the caret under the right character is not obvious -
//! tabs, characters wider than a byte, a line too long to print, and the end of
//! the input.

use winnow_grammar::span::{caret, SpanExt};

/// The two lines a compiler prints: the line, and what part of it.
#[test]
fn line_and_caret() {
    let src = "struct S {\n    id: i32\n    temp: f64,\n}\n";
    assert_eq!(
        caret(src, 27, 4),
        concat!("   3 |     temp: f64,\n", "           ^^^^"),
    );
}

/// A tab is copied as a tab, not measured as one space.
///
/// The width a terminal gives a tab is the terminal's business; reproducing the
/// line's own leading characters is the only alignment that survives whatever
/// it decides.
#[test]
fn tabs_are_reproduced() {
    let src = "fn f() {\n\t\tx = ;\n}\n";
    let out = caret(src, 15, 1);
    let (_, carets) = out.split_once('\n').unwrap();
    assert_eq!(carets, "       \t\t    ^");
}

/// The column counts characters, so a caret after a non-ASCII word still lands
/// on the right one.
#[test]
fn multibyte_characters_count_once() {
    let src = "let café = ;\n";
    let offset = src.find(';').unwrap();
    let out = caret(src, offset, 1);
    let (text, carets) = out.split_once('\n').unwrap();
    let at = carets.chars().position(|c| c == '^').unwrap();
    assert_eq!(text.chars().nth(at), Some(';'));
}

/// A line too long to print is windowed around the position, and the cut ends
/// say so. A minified document is one line; a caret four thousand columns to
/// the right of a terminal is not a diagnostic.
#[test]
fn a_long_line_is_windowed_around_the_caret() {
    let src = format!("{}X{}", "a".repeat(200), "b".repeat(200));
    let out = caret(&src, 200, 1);
    let (text, carets) = out.split_once('\n').unwrap();

    assert!(text.contains('…') && text.ends_with('…'), "{text}");
    assert!(text.chars().count() <= 96 + "   1 | ".len() + 2, "{text}");

    let at = carets.chars().position(|c| c == '^').unwrap();
    assert_eq!(text.chars().nth(at), Some('X'));
}

/// … and never scrolls past an end it does not need to: a position near the
/// start of a long line keeps the start visible.
#[test]
fn a_long_line_is_not_scrolled_when_the_caret_is_at_its_start() {
    let src = format!("abc{}", "b".repeat(300));
    let out = caret(&src, 1, 1);
    let (text, carets) = out.split_once('\n').unwrap();

    assert!(text.starts_with("   1 | abc"), "{text}");
    let at = carets.chars().position(|c| c == '^').unwrap();
    assert_eq!(text.chars().nth(at), Some('b'));
}

/// At the end of the input there is no character to point at, and the caret
/// goes where the missing one would be.
#[test]
fn end_of_input_points_past_the_last_character() {
    let src = "fn f() {\n";
    let out = caret(src, src.len(), 1);
    assert_eq!(out, concat!("   2 | \n", "       ^"));
}

/// A `\r\n` line ending is not part of the line.
#[test]
fn carriage_return_is_not_printed() {
    let src = "let x = 1;\r\nlet y = ;\r\n";
    let out = caret(src, src.rfind(';').unwrap(), 1);
    let (text, _) = out.split_once('\n').unwrap();
    assert_eq!(text, "   2 | let y = ;");
}

/// A caret wider than what is left of the line stops at the line's end rather
/// than running into the next one.
#[test]
fn the_caret_stops_at_the_end_of_the_line() {
    let src = "ab\ncd\n";
    let out = caret(src, 1, 40);
    assert_eq!(out, concat!("   1 | ab\n", "        ^"));
}

/// A span carries its own width, so a diagnostic about one underlines the whole
/// of it.
#[test]
fn a_span_underlines_what_it_covers() {
    let src = "alpha\nbeta gamma\n";
    assert_eq!(
        (11..16).caret(src),
        concat!("   2 | beta gamma\n", "            ^^^^^"),
    );
}

/// A span across a line break underlines to the end of the first line: that is
/// where the reader has to look, and printing the second line under a message
/// about the first helps nobody.
#[test]
fn a_span_across_lines_stops_at_the_first() {
    let src = "alpha\nbeta\n";
    assert_eq!((3..8).caret(src), concat!("   1 | alpha\n", "          ^^"));
}

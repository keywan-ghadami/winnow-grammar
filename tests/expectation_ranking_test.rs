//! What a message names when several parsers failed at one position.
//!
//! Two axes, and both are needed. A **requirement** outranks an **optional
//! continuation** - a repetition that had already met its minimum, or an
//! `x?` - and among optional continuations, anything outranks the implicit
//! whitespace skip. Neither axis drops an expectation: what loses becomes
//! the `note:` line, because a repetition's reason for stopping is often
//! the useful half.

use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar Doc {
        pub rule doc -> usize = xs:item* "." -> { xs.len() }
        rule item -> u32 = n:u32 -> { n } | "#" _h:hex_digit1 -> { 0 }
    }
}

grammar! {
    grammar Fields {
        WSE = multispace1
        WS = (WSE | COMMENT)*
        COMMENT = "//" until(line_ending)

        pub rule fields -> usize = _h:field tail:field_tail* "}" -> { tail.len() + 1 }
        rule field -> u32 = _n:ident ":" v:u32 -> { v }
        rule field_tail -> u32 = "," f:field -> { f }
    }
}

/// The `.` is required here and another item is not, so the `.` is what the
/// message names - and the items are still named, under it.
#[test]
fn a_requirement_outranks_an_optional_continuation() {
    let e = Doc::parse_doc().parse_test("1 2 x").inner.unwrap_err();
    assert!(
        e.starts_with("expected `.`; found unexpected token `x`"),
        "{e}"
    );
    assert!(
        e.contains("note: also possible here: `#`, integer literal"),
        "{e}"
    );
}

/// A grammar that supports comments has to write `WS = (WSE | COMMENT)*`,
/// which fails with two expectations wherever a token is missing. Those two
/// used to be the whole message: the `}` and the `,` the reader needed were
/// discarded by the aggregation priority of the whitespace skip.
#[test]
fn the_whitespace_skip_does_not_speak_for_the_grammar() {
    let e = Fields::parse_fields()
        .parse_test("a: 1\n b: 2 }")
        .inner
        .unwrap_err();
    assert!(
        e.starts_with("expected `}`; found unexpected token `b`"),
        "{e}"
    );
    // Nothing is dropped except whitespace, which no reader can act on: the
    // skip is greedy, so supplying more only moves the same failure along.
    assert!(e.contains("note: also possible here: `,`, `//`"), "{e}");
    assert!(!e.contains("whitespace"), "{e}");
}

/// The comment form survives as a note, because a comment *can* be what the
/// author meant - a `/` where `//` belongs is a real mistake, and unlike
/// whitespace it is worth naming.
#[test]
fn a_comment_is_named_but_does_not_lead() {
    let e = Fields::parse_fields()
        .parse_test("a: 1\n /oops\n}")
        .inner
        .unwrap_err();
    assert!(e.contains("`//`"), "{e}");
    assert!(!e.starts_with("expected one of: `//`"), "{e}");
}

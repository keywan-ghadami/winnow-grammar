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

grammar! {
    grammar Program {
        pub rule program -> usize = items:item* -> { items.len() }
        rule item -> usize = "fn" "(" ")" "{" stmts:stmt* "}" -> { stmts.len() }
        rule stmt -> usize = "let" n:u32 ";" -> { n as usize }
    }
}

/// An element that *began* is not an optional continuation, wherever the
/// repetition around it had met its minimum.
///
/// `program = item*` makes every item optional, so before this the whole of
/// what an unfinished item knew - including the `}` it was missing - was
/// recorded as "something else could have gone here" and lost to whatever else
/// was possible at that offset. The item did not merely fail to start: it read
/// four tokens and a statement, and the brace is what it needs.
#[test]
fn an_element_that_began_is_a_requirement() {
    let e = Program::parse_program()
        .parse_test("fn(){let 1;")
        .inner
        .unwrap_err();
    assert!(
        e.starts_with("unexpected end of input, expected `}`"),
        "{e}"
    );
    // Another statement was possible there too, and says less.
    assert!(e.contains("note: also possible here: `let`"), "{e}");
}

/// The other half, and the reason the check is against the end of the trivia
/// the element skipped rather than against its start: an item that failed on
/// the blank before it has consumed nothing of its own.
#[test]
fn an_element_that_only_skipped_whitespace_did_not_begin() {
    // The second `item` fails at `x`, one blank past where it was tried. What
    // the reader needs is that the *program* ends there, not what an item
    // could have started with.
    let e = Program::parse_program()
        .parse_test("fn(){} x")
        .inner
        .unwrap_err();
    assert!(e.contains("`fn`"), "{e}");
    // …and it is a note, not the headline: the trailing input is the failure.
    assert!(!e.starts_with("expected `fn`"), "{e}");
}

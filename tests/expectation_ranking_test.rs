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

grammar! {
    grammar Calls {
        // A file of blocks, because that is the shape the ranking is about:
        // the outer repetition is what records an unfinished block's error.
        pub rule doc -> usize = bs:block* -> { bs.len() }
        rule block -> usize = "{" xs:stmt* "}" -> { xs.len() }
        rule stmt -> usize =
              n:name "(" ")" -> { n.len() }
            | n:name "{" "}" -> { n.len() }
            | n:name -> { n.len() }
        rule name -> &'a str = s:raw_ident -> { s }
    }
}

/// What a *losing* alternative found is kept when it had begun.
///
/// `alt` throws away the errors of the alternatives before the winning one.
/// Usually that is right - they failed where they started and said nothing new.
/// Where a shorter alternative succeeds and a longer one failed after reading
/// tokens, it is how the useful message disappears: here `a` parses as a bare
/// name, so the two alternatives that read the name and wanted a `(` or a `{`
/// are abandoned, and without recording them nothing at all is known about the
/// position after `a`.
#[test]
fn an_alternative_that_began_is_kept_when_a_shorter_one_wins() {
    let e = Calls::parse_doc().parse_test("{ a").inner.unwrap_err();
    // The `}` the block is missing - not the `(` or `{` a call could have had.
    assert!(
        e.starts_with("unexpected end of input, expected `}`"),
        "{e}"
    );
    assert!(e.contains("`(`") && e.contains("`{`"), "{e}");
}

/// Two requirements at one position are told apart by how long each has been
/// open.
///
/// The block started at the first character and is still missing its `}`; the
/// alternative that read `a` and hoped for a `(` started two characters ago.
/// Without this the guess wins, because two expectations beat one on priority -
/// the flaw the ranking removed for optional continuations and which applies
/// again as soon as both sides are requirements.
#[test]
fn the_requirement_that_has_been_open_longest_leads() {
    let e = Calls::parse_doc().parse_test("{ a").inner.unwrap_err();
    assert!(
        e.starts_with("unexpected end of input, expected `}`"),
        "{e}"
    );
    assert!(
        !e.starts_with("unexpected end of input, expected one of"),
        "{e}"
    );
}

grammar! {
    grammar Bounds {
        pub rule doc -> usize = "x" b:bound? "!" -> { b.unwrap_or(0) }
        // A brace group is a bound only when it starts with a digit - the
        // lookahead is how a grammar tells the two apart.
        rule bound -> usize = peek(("{" digit)) "{" n:usize "}" -> { n }
    }
}

/// What fails inside a `peek(…)` is a test that said no, not an expectation.
///
/// The lookahead consumes nothing and demands nothing: an alternative whose
/// lookahead fails simply does not apply. Recorded, it puts the *test* in the
/// message - every brace group that is not a bound would report
/// `expected a digit` - and it wins on progress, because a lookahead is tried
/// one token further along than the thing that actually belongs there.
#[test]
fn a_failing_lookahead_is_not_an_expectation() {
    let e = Bounds::parse_doc().parse_test("x{a}").inner.unwrap_err();
    assert!(e.starts_with("expected `!`"), "{e}");
    assert!(!e.contains("digit"), "{e}");
}

/// And the lookahead still does its work: a brace group that *is* a bound is
/// one, and the rule that reads it is entered.
#[test]
fn the_lookahead_still_decides_the_alternative() {
    Bounds::parse_doc().parse_test("x{7}!").assert_success_is(7);
    Bounds::parse_doc().parse_test("x!").assert_success_is(0);
}

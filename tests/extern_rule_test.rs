//! `extern rule`: a hand-written parser declared in the grammar.
//!
//! The shape SYNTAX.md documents, pinned - the signature above all, which is
//! winnow's own over `ParseInput` returning this crate's `ParseError` (not
//! `ErrMode<ParseError>`, not the grammar's error type parameter).

use winnow::token::take_till;
use winnow::Parser;
use winnow_grammar::error::ParseError;
use winnow_grammar::testing::WinnowTestExt;
use winnow_grammar::{grammar, ParseInput, Symbol};

fn city<'a, S: Clone + std::fmt::Debug>(i: &mut ParseInput<'a, S>) -> Result<Symbol, ParseError> {
    let s: &str = take_till(1.., ';').parse_next(i)?;
    // The context is reachable from a hand-written parser like from any
    // generated one - this is the third path to the interner (ADR 18).
    Ok(i.state.interner.intern_string(s))
}

grammar! {
    grammar Cities {
        extern rule city -> Symbol;

        pub row -> (Symbol, i32) = c:city ";" t:i32 -> { (c, t) }
    }
}

#[test]
fn a_hand_written_parser_reaches_the_interner() {
    Cities::parse_row()
        .parse_test("São Paulo;-12")
        .assert_success_with(|(c, t), ctx| {
            assert_eq!(ctx.interner.resolve(*c), "São Paulo");
            assert_eq!(*t, -12);
        });
}

#[test]
fn its_symbols_are_the_grammars_symbols() {
    // `city` and `intern` write into the same interner, so the same text is
    // the same symbol whichever side produced it.
    Cities::parse_row()
        .parse_test("Hamburg;1")
        .assert_success_with(|(c, _), ctx| {
            let same = ctx.interner.intern_string("Hamburg");
            assert_eq!(*c, same);
        });
}

#[test]
fn a_failing_hand_written_parser_fails_the_rule() {
    Cities::parse_row().parse_test(";1").assert_failure();
}

// -----------------------------------------------------------------------------
// A doc comment is an attribute, and `ExternRule::parse` reads attributes
// before the `extern` keyword. The grammar body has to route the declaration
// there even when an attribute stands in front of it.
// -----------------------------------------------------------------------------

fn town<'a, S: Clone + std::fmt::Debug>(i: &mut ParseInput<'a, S>) -> Result<Symbol, ParseError> {
    let s: &str = take_till(1.., ';').parse_next(i)?;
    Ok(i.state.interner.intern_string(s))
}

grammar! {
    grammar Documented {
        /// The town parser lives next to the grammar.
        extern rule town -> Symbol;

        pub row -> (Symbol, i32) = c:town ";" t:i32 -> { (c, t) }
    }
}

#[test]
fn an_extern_rule_may_carry_a_doc_comment() {
    Documented::parse_row()
        .parse_test("Hamburg;12")
        .assert_success_with(|(c, t), ctx| {
            assert_eq!(ctx.interner.resolve(*c), "Hamburg");
            assert_eq!(*t, 12);
        });
}

//! The shared interner of ADR 14, across the pieces of a `par_fold` rule.
//!
//! `rt::parse_piece` calls `new_context()` once per piece, so what that
//! closure returns decides whether the pieces share an interner. Both answers
//! parse; only one of them produces symbols that mean the same thing in every
//! piece. Until this file, every call site in the tests and in `SYNTAX.md`
//! passed `ParseContext::<()>::default` - the answer that does not share.

use winnow_grammar::rt::Parallelism;
use winnow_grammar::{grammar, InternerContext, ParseContext, Symbol};

grammar! {
    grammar Cities {
        // No implicit whitespace: a row is exactly `name;digits\n`.
        WS -> () = "" -> { () }

        #[frame(boundary = "\n")]
        pub ROW -> Symbol = n:ident ";" digit1 frame_end -> { n }

        pub FILE -> Vec<Symbol> = s:par_fold(
            ROW,
            Vec::new,
            |mut acc: Vec<Symbol>, n: Symbol| { acc.push(n); acc },
            |mut a: Vec<Symbol>, b: Vec<Symbol>| { a.extend(b); a }
        ) -> { s }
    }
}

/// The worked example: the caller owns the interner and the closure clones it
/// into every piece, as ADR 16 §3 describes. Symbols are then comparable
/// across pieces and resolve against the interner the caller kept.
#[test]
fn one_interner_shared_by_every_piece() {
    let input = "Hamburg;1\nZurich;2\nZurich;3\nHamburg;4\n";

    let interner = InternerContext::new();
    let new_context = {
        let interner = interner.clone();
        move || ParseContext::<()> {
            interner: interner.clone(),
            ..Default::default()
        }
    };

    let syms = Cities::parse_FILE_pieces(input, new_context, Parallelism::Pieces(2)).unwrap();

    let texts: Vec<&str> = syms.iter().map(|s| interner.resolve(*s)).collect();
    assert_eq!(texts, ["Hamburg", "Zurich", "Zurich", "Hamburg"]);
    // The same city in two different pieces is the same symbol.
    assert_eq!(syms[0], syms[3]);
    assert_eq!(syms[1], syms[2]);
    assert_ne!(syms[0], syms[1]);
}

/// The trap, pinned. A closure that builds a *fresh* interner per piece -
/// which `ParseContext::default` does - numbers each piece from one. Nothing
/// fails: the symbols are well-formed, resolvable inside their own piece, and
/// wrong everywhere else.
///
/// This test asserts the broken comparisons on purpose. Should a later change
/// make the ids meaningful across pieces (or reject the mismatch), this test
/// is what says so.
#[test]
fn a_fresh_interner_per_piece_makes_symbols_incomparable() {
    // Cut as ["Hamburg;1\nZurich;2\n", "Zurich;3\nZurich;4\n"], so piece 2
    // numbers "Zurich" 1 - the id piece 1 gave "Hamburg".
    let input = "Hamburg;1\nZurich;2\nZurich;3\nZurich;4\n";

    let syms =
        Cities::parse_FILE_pieces(input, ParseContext::<()>::default, Parallelism::Pieces(2))
            .unwrap();

    // Two different cities compare equal ...
    assert_eq!(syms[0], syms[2], "Hamburg and Zurich share an id");
    // ... and one city compares unequal to itself.
    assert_ne!(syms[1], syms[2], "the two Zurichs differ");
}

//! One context, several parses - ADR 19 §1.
//!
//! ADR 14 built `ParseContext` so that the interner outlives a single parse
//! and is shared across source files. The same struct also carries the
//! diagnostics engine's working space (`furthest`, `rules`, `fold`), and
//! nothing reset it, so a second parse through one context inherited the
//! first one's. The visible effect was narrow and real: `fold.base` numbers a
//! `par_fold`'s items and is advanced when a parse fails, so the *next*
//! parse's message counted from the previous total - `in item 7` for what a
//! fresh context called `in item 4`, growing by three with every failure.
//!
//! These tests fix the boundaries of that, in both directions: what must be
//! carried between parses (the interner) and what must not (everything the
//! engine writes).

use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::{grammar, InternerContext, ParseContext, ParseInput, Symbol};

grammar! {
    grammar Par {
        WS -> () = "" -> { () }

        #[frame(boundary = "\n")]
        pub ITEM -> i32 = v:i32 "\n" -> { v }

        pub FILE -> i32 = s:par_fold(
            ITEM,
            || 0i32,
            |a: i32, b: i32| a + b,
            |a: i32, b: i32| a + b
        ) -> { s }
    }
}

grammar! {
    grammar Plain {
        WS -> () = "" -> { () }
        rule ITEM -> i32 = v:i32 "\n" -> { v }
        // A plain `fold` is untracked - it never reads `fold.base`. The
        // control for the test below.
        pub FILE -> i32 = s:fold(ITEM, || 0i32, |a: i32, b: i32| a + b) -> { s }
    }
}

grammar! {
    grammar Names {
        pub two -> (Symbol, Symbol) = a:ident b:ident -> { (a, b) }
    }
}

const GOOD: &str = "1\n2\n3\n";
const BAD: &str = "1\n2\n3\nx\n";

/// Parse through a context the caller owns and get it back - the shape ADR
/// 14's example describes, and the only way to reuse one.
fn par(ctx: ParseContext<()>, input: &str) -> (Result<i32, String>, ParseContext<()>) {
    let mut stream = ParseInput {
        input: LocatingSlice::new(input),
        state: ctx,
    };
    let r = Par::parse_FILE()
        .parse_next(&mut stream)
        .map_err(|e| e.render(input));
    (r, stream.state)
}

fn plain(ctx: ParseContext<()>, input: &str) -> (Result<i32, String>, ParseContext<()>) {
    let mut stream = ParseInput {
        input: LocatingSlice::new(input),
        state: ctx,
    };
    let r = Plain::parse_FILE()
        .parse_next(&mut stream)
        .map_err(|e| e.render(input));
    (r, stream.state)
}

fn message(r: Result<i32, String>) -> String {
    r.expect_err("this input does not parse")
}

#[test]
fn the_same_input_gets_the_same_message_however_often_the_context_was_used() {
    let fresh = message(par(ParseContext::default(), BAD).0);
    assert!(
        fresh.contains("in item 4"),
        "the fourth item is the one that fails: {fresh}"
    );

    // Four parses through one context, two of which fail - the case that
    // advanced `fold.base` and offset everything after it.
    let (_, ctx) = par(ParseContext::default(), GOOD);
    let (first_failure, ctx) = par(ctx, BAD);
    let (_, ctx) = par(ctx, GOOD);
    let (second_failure, ctx) = par(ctx, BAD);
    let (third_failure, _) = par(ctx, BAD);

    assert_eq!(message(first_failure), fresh);
    assert_eq!(message(second_failure), fresh);
    assert_eq!(
        message(third_failure),
        fresh,
        "the offset used to grow with every failure: item 4, then 7, then 10"
    );
}

#[test]
fn a_parse_starts_from_a_clean_engine_whatever_it_is_handed() {
    // The reset happens where a parse *begins*, so this tests it directly
    // rather than through a previous parse: a context carrying junk in the
    // engine's fields - which is exactly what a failed `par_fold` used to
    // leave behind - must not change what the next parse reports.
    let fresh = message(par(ParseContext::default(), BAD).0);

    let junk = ParseContext::<()> {
        fold: winnow_grammar::FoldProgress {
            base: 41,
            seen: 7,
            at: 999,
        },
        rules: vec!["not_a_rule_of_this_grammar", "nor_this"],
        furthest: None,
        ..Default::default()
    };
    assert_eq!(message(par(junk, BAD).0), fresh);
}

#[test]
fn the_leftover_of_a_failed_parse_stops_accumulating() {
    // A failed `par_fold` does leave `fold.base` set - that is `entry_framed`
    // writing where its replay has to start, and it is not cleaned up
    // afterwards. What must not happen is that the *next* parse adds to it:
    // the leftover used to grow 3, 6, 9 with every failure, and each step
    // shifted the item number in the message. Now every failure leaves the
    // same value, because each parse starts from a reset.
    let mut ctx = ParseContext::<()>::default();
    let mut bases = Vec::new();
    for _ in 0..3 {
        let (r, next) = par(ctx, BAD);
        assert!(r.is_err());
        bases.push(next.fold.base);
        ctx = next;
    }
    assert_eq!(bases, [3, 3, 3], "the leftover is bounded, not cumulative");
    assert!(ctx.rules.is_empty(), "the live rule stack is balanced");
}

#[test]
fn what_a_parse_accepts_never_depended_on_it() {
    // The defect was confined to the message. Pinning that keeps a future
    // change to `begin_parse` from being mistaken for a parsing change.
    let (fresh, _) = par(ParseContext::default(), GOOD);
    let (_, ctx) = par(ParseContext::default(), BAD);
    let (after_failure, _) = par(ctx, GOOD);
    assert_eq!(fresh.unwrap(), 6);
    assert_eq!(after_failure.unwrap(), 6);
}

#[test]
fn a_plain_fold_was_never_affected() {
    // `fold` (not `par_fold`) runs untracked, so it never read the base. The
    // control: if this ever starts differing, the cause is not the base.
    let fresh = message(plain(ParseContext::default(), BAD).0);
    let (r, ctx) = plain(ParseContext::default(), BAD);
    assert!(r.is_err());
    assert_eq!(message(plain(ctx, BAD).0), fresh);
}

#[test]
fn the_interner_is_the_thing_that_does_carry_over() {
    // The other direction, and the point of reusing a context at all: what
    // ADR 14 wants kept is kept. `begin_parse` must not touch it.
    let interner = InternerContext::new();
    let mut ctx = ParseContext::<()> {
        interner: interner.clone(),
        ..Default::default()
    };

    let mut symbols = Vec::new();
    for input in ["alpha beta", "beta gamma"] {
        let mut stream = ParseInput {
            input: LocatingSlice::new(input),
            state: ctx,
        };
        let (a, b) = Names::parse_two().parse_next(&mut stream).unwrap();
        symbols.push((a, b));
        ctx = stream.state;
    }

    let (alpha, beta1) = symbols[0];
    let (beta2, gamma) = symbols[1];
    assert_eq!(beta1, beta2, "the same word in two parses is one symbol");
    assert_ne!(alpha, gamma);
    assert_eq!(interner.len(), 3, "alpha, beta, gamma");
    assert_eq!(ctx.interner.resolve(alpha), "alpha");
}

use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;
use winnow_grammar::Symbol;

grammar! {
    grammar InterningTest {
        // A rule that parses two identifiers and returns them as a tuple.
        // This allows us to get two symbols from the same parse run (and same interner).
        pub two_idents -> (Symbol, Symbol) = i1:ident i2:ident -> { (i1, i2) }
    }
}

#[test]
fn test_interning_equality() {
    let input = "hello hello";
    // The `parse_test` extension creates a default `ParseContext` for the test run.
    InterningTest::parse_two_idents()
        .parse_test(input)
        .assert_success_with(|(s1, s2), _state| {
            // Within the same parse run, the same string should produce the same symbol.
            assert_eq!(
                s1, s2,
                "The same identifier string should result in the same Symbol"
            );
        });
}

#[test]
fn test_interning_uniqueness() {
    let input = "hello world";
    InterningTest::parse_two_idents()
        .parse_test(input)
        .assert_success_with(|(s1, s2), _state| {
            // Different strings should produce different symbols.
            assert_ne!(
                s1, s2,
                "Different identifier strings should result in different Symbols"
            );
        });
}

#[test]
fn test_resolve_returns_the_interned_text() {
    // Regression: `Symbol`'s round-trip used to add one twice, so `resolve`
    // returned the *next* symbol's text. Comparing symbols to each other (the
    // tests above) cannot catch that - only resolving can.
    let input = "alpha beta";
    InterningTest::parse_two_idents()
        .parse_test(input)
        .assert_success_with(|(s1, s2), state| {
            assert_eq!(state.interner.resolve(*s1), "alpha");
            assert_eq!(state.interner.resolve(*s2), "beta");
        });
}

#[test]
fn test_resolve_the_last_interned_symbol() {
    // The off-by-one made the most recently interned symbol point one past the
    // end, so this panicked with "Key out of bounds" rather than returning text.
    let interner = winnow_grammar::InternerContext::new();
    let only = interner.intern_string("solo");
    assert_eq!(interner.resolve(only), "solo");
}

#[test]
fn test_resolve_round_trips_many_symbols() {
    let interner = winnow_grammar::InternerContext::new();
    let words = ["fn", "main", "println", "x", "fn"];

    let symbols: Vec<_> = words.iter().map(|w| interner.intern_string(w)).collect();

    for (symbol, expected) in symbols.iter().zip(words.iter()) {
        assert_eq!(interner.resolve(*symbol), *expected);
    }

    // Repeated text must still collapse onto one symbol.
    assert_eq!(symbols[0], symbols[4]);
}

// -----------------------------------------------------------------------------
// `Symbol::index` - the dense number, and what a caller does with it.
// -----------------------------------------------------------------------------

#[test]
fn indices_are_dense_and_in_first_seen_order() {
    let interner = winnow_grammar::InternerContext::new();
    assert!(interner.is_empty());

    let a = interner.intern_string("alpha");
    let b = interner.intern_string("beta");
    let a2 = interner.intern_string("alpha");
    let c = interner.intern_string("gamma");

    assert_eq!(a.index(), 0);
    assert_eq!(b.index(), 1);
    assert_eq!(a2.index(), 0, "the same string keeps its index");
    assert_eq!(c.index(), 2);

    assert_eq!(interner.len(), 3);
    assert!(!interner.is_empty());
}

#[test]
fn the_index_addresses_a_parallel_vec() {
    // The aggregation shape: the symbol is the row in the caller's own table,
    // so there is no second lookup - `TODO.md` §6b.
    let interner = winnow_grammar::InternerContext::new();
    let mut totals: Vec<i64> = Vec::new();

    for (city, temp) in [
        ("Hamburg", 12i64),
        ("Zürich", 20),
        ("Hamburg", 8),
        ("東京", 30),
        ("Zürich", -4),
    ] {
        let i = interner.intern_string(city).index() as usize;
        if i >= totals.len() {
            totals.resize(i + 1, 0);
        }
        totals[i] += temp;
    }

    assert_eq!(totals.len(), interner.len());
    let by_name = |name: &str| totals[interner.intern_string(name).index() as usize];
    assert_eq!(by_name("Hamburg"), 20);
    assert_eq!(by_name("Zürich"), 16);
    assert_eq!(by_name("東京"), 30);

    // And the way back: `resolve` names the row, so a report can be printed
    // by walking the table rather than the input.
    let named: Vec<(&str, i64)> = ["Hamburg", "Zürich", "東京"]
        .into_iter()
        .map(|n| {
            let sym = interner.intern_string(n);
            (interner.resolve(sym), totals[sym.index() as usize])
        })
        .collect();
    assert_eq!(named, [("Hamburg", 20), ("Zürich", 16), ("東京", 30)]);
}

#[test]
fn a_parse_hands_out_indices_a_caller_can_aggregate_with() {
    InterningTest::parse_two_idents()
        .parse_test("hamburg zurich")
        .assert_success_with(|(a, b), ctx| {
            assert_eq!(a.index(), 0);
            assert_eq!(b.index(), 1);
            assert_eq!(ctx.interner.len(), 2);
            assert_eq!(ctx.interner.resolve(*a), "hamburg");
        });
}

// -----------------------------------------------------------------------------
// The lookup cache in front of the interner (`TODO.md` §4). It is an
// optimisation and must therefore be invisible: the same symbols, for every
// input that could tell the two paths apart.
// -----------------------------------------------------------------------------

use winnow_grammar::{InternerContext, ParseContext};

/// The interner's answer is the answer. Anything the cache does differently is
/// a bug, so the test asks both for every word.
fn agrees(words: &[&str]) {
    let mut ctx = ParseContext::<()>::default();
    let reference = InternerContext::new();

    for w in words {
        let cached = ctx.intern(w);
        let direct = reference.intern_string(w);
        assert_eq!(
            cached.index(),
            direct.index(),
            "{w:?} got a different number through the cache"
        );
        assert_eq!(ctx.interner.resolve(cached), *w);
    }
    assert_eq!(ctx.interner.len(), reference.len());
}

#[test]
fn the_cache_returns_what_the_interner_would() {
    agrees(&["alpha", "beta", "alpha", "gamma", "beta", "alpha"]);
}

#[test]
fn words_that_share_their_first_eight_bytes_are_not_confused() {
    // The cache's tag is the first eight bytes and the length. These words
    // share both prefix and length, so only the verification against the
    // interned text can tell them apart - which is why texts longer than
    // eight bytes are verified.
    agrees(&[
        "customer_id",
        "customer_ip",
        "customer_id",
        "customer_ip",
        "identifier_0001",
        "identifier_0002",
        "identifier_0001",
    ]);
}

#[test]
fn a_slot_that_gets_displaced_is_simply_interned_again() {
    // 4000 distinct words through 512 slots: every slot is overwritten many
    // times, and nothing may be lost by it.
    let words: Vec<String> = (0..4000).map(|i| format!("w{i}")).collect();
    let mut ctx = ParseContext::<()>::default();

    let first: Vec<_> = words.iter().map(|w| ctx.intern(w)).collect();
    let again: Vec<_> = words.iter().map(|w| ctx.intern(w)).collect();

    assert_eq!(first, again, "a second pass must return the same symbols");
    assert_eq!(ctx.interner.len(), words.len());
    for (w, s) in words.iter().zip(&first) {
        assert_eq!(ctx.interner.resolve(*s), w.as_str());
    }
}

#[test]
fn the_cache_does_not_survive_a_change_of_interner() {
    // Two interners number from zero independently. A context whose interner
    // is replaced must not answer from the old one's numbers; `begin_parse`
    // is where that is noticed, and every parse calls it.
    let a = InternerContext::new();
    let b = InternerContext::new();
    a.intern_string("first"); // so that "shared" gets 1 in `a` and 0 in `b`

    let mut ctx = ParseContext::<()> {
        interner: a.clone(),
        ..Default::default()
    };
    ctx.begin_parse();
    let in_a = ctx.intern("shared");
    assert_eq!(in_a.index(), 1);

    ctx.interner = b.clone();
    ctx.begin_parse();
    let in_b = ctx.intern("shared");
    assert_eq!(in_b.index(), 0, "b numbers from zero");
    assert_eq!(b.resolve(in_b), "shared");
}

#[test]
fn a_cloned_context_answers_for_itself() {
    // The clone starts with an empty cache - a piece of a `par_fold` has
    // parsed nothing yet - but the interner is shared, so the symbols agree.
    let mut ctx = ParseContext::<()>::default();
    let alpha = ctx.intern("alpha");

    let mut piece = ctx.clone();
    piece.begin_parse();
    assert_eq!(piece.intern("alpha"), alpha);
    assert_eq!(piece.intern("beta").index(), 1);
    assert_eq!(ctx.interner.len(), 2, "one interner behind both");
}

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

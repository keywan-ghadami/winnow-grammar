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

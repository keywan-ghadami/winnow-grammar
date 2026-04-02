use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;
use winnow_grammar::Symbol;
use winnow_grammar::interner::StringInterner;

// Define a state struct that contains the interner.
#[derive(Clone, Default, Debug)]
pub struct TestState {
    pub interner: StringInterner,
}

grammar! {
    grammar InterningTest for TestState {
        // A rule that parses two identifiers and returns them as a tuple.
        // This allows us to get two symbols from the same parse run (and same interner).
        pub two_idents -> (Symbol, Symbol) = i1:ident " " i2:ident -> { (i1, i2) }
    }
}

#[test]
fn test_interning_equality() {
    let input = "hello hello";
    // Use the `TestState` in the parser call.
    InterningTest::parse_two_idents()
        .parse_test(input)
        .assert_success_with(|(s1, s2), _state| {
            // Within the same parse run, the same string should produce the same symbol.
            assert_eq!(s1, s2, "The same identifier string should result in the same Symbol");
        });
}

#[test]
fn test_interning_uniqueness() {
    let input = "hello world";
    // Use the `TestState` in the parser call.
    InterningTest::parse_two_idents()
        .parse_test(input)
        .assert_success_with(|(s1, s2), _state| {
            // Different strings should produce different symbols.
            assert_ne!(s1, s2, "Different identifier strings should result in different Symbols");
        });
}

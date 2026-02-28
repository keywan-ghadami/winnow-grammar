use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;

grammar! {
    grammar SpaceParser {
        // Disable automatic whitespace skipping by overriding ws with empty
        use winnow::combinator::empty;
        rule ws -> () = empty -> { () }

        pub rule test_space0 -> String =
            s:space0 -> { s }
        pub rule test_space1 -> String =
            s:space1 -> { s }
    }
}

#[test]
fn test_space_literal() {
    // Note: With ws disabled, space0 and space1 will consume the spaces.
    // However, if automatic whitespace skipping was enabled (the default), `ws` would consume the spaces *before* `space0` runs.
    // Since `space0` matches 0 or more, it would then match empty string.
    // By disabling `ws`, `space0` sees the spaces.

    let input = LocatingSlice::new("   ");
    let result = SpaceParser::parse_test_space0.parse(input).unwrap();
    assert_eq!(result, "   ");

    let input = LocatingSlice::new("");
    let result = SpaceParser::parse_test_space0.parse(input).unwrap();
    assert_eq!(result, "");

    let input = LocatingSlice::new("   ");
    let result = SpaceParser::parse_test_space1.parse(input).unwrap();
    assert_eq!(result, "   ");

    let input = LocatingSlice::new("");
    let result = SpaceParser::parse_test_space1.parse(input);
    assert!(result.is_err());
}

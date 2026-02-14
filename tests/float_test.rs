use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;

grammar! {
    grammar FloatParser {
        pub rule test_float -> f64 =
            f:float -> { f }
    }
}

#[test]
fn test_float_literal() {
    let input = LocatingSlice::new("3.14");
    let result = FloatParser::parse_test_float.parse(input).unwrap();
    assert!((result - 3.14).abs() < f64::EPSILON);

    let input = LocatingSlice::new("-0.5");
    let result = FloatParser::parse_test_float.parse(input).unwrap();
    assert!((result - -0.5).abs() < f64::EPSILON);

    let input = LocatingSlice::new("123");
    let result = FloatParser::parse_test_float.parse(input).unwrap();
    assert!((result - 123.0).abs() < f64::EPSILON);

    let input = LocatingSlice::new("abc");
    let result = FloatParser::parse_test_float.parse(input);
    assert!(result.is_err());
}

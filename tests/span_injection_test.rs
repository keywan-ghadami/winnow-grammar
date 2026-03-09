use grammar_kit::WithSpan;
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::grammar;

#[derive(Debug, PartialEq, Clone)]
pub struct SpannedInt {
    pub val: u32,
    pub span: std::ops::Range<usize>,
}

impl WithSpan<u32> for SpannedInt {
    fn with_span(val: u32, span: std::ops::Range<usize>) -> Self {
        Self { val, span }
    }
}

grammar! {
    grammar SpanTest {
        // We now need @= to opt-in for span injection
        pub rule main -> SpannedInt @= n:u32
    }
}

#[test]
fn test_span_injection() {
    let input = "  42  ";
    let input = LocatingSlice::new(input);
    let result = SpanTest::parse_main.parse(input).unwrap();

    assert_eq!(result.val, 42);
    assert_eq!(result.span, 2..4);
}

#[derive(Debug, PartialEq, Clone)]
pub struct SpannedTuple {
    pub a: u32,
    pub b: u32,
    pub span: std::ops::Range<usize>,
}

impl WithSpan<(u32, u32)> for SpannedTuple {
    fn with_span(val: (u32, u32), span: std::ops::Range<usize>) -> Self {
        Self {
            a: val.0,
            b: val.1,
            span,
        }
    }
}

grammar! {
    grammar SpanTupleTest {
        // Opt-in with @=
        pub rule main -> SpannedTuple @= a:u32 b:u32
    }
}

#[test]
fn test_span_injection_tuple() {
    let input = " 10 20 ";
    let input = LocatingSlice::new(input);
    let result = SpanTupleTest::parse_main.parse(input).unwrap();

    assert_eq!(result.a, 10);
    assert_eq!(result.b, 20);
    assert_eq!(result.span, 1..6);
}

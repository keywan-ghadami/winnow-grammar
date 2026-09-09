use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

#[derive(Debug, PartialEq)]
pub struct CustomNode {
    pub name: String,
    pub span: std::ops::Range<usize>,
}

impl CustomNode {
    fn from_data(name: String, span: std::ops::Range<usize>) -> Self {
        Self { name, span }
    }
}

grammar! {
    grammar ExplicitSpanTest {
        pub custom_node -> CustomNode @= a:raw_ident -> { CustomNode::from_data(a.to_string(), _span) }
    }
}

#[test]
fn test_explicit_span_injection() {
    let input = "  my_ident  ";
    let result = ExplicitSpanTest::parse_custom_node()
        .parse_test(input)
        .inner
        .unwrap();

    assert_eq!(result.name, "my_ident");
    // "  my_ident  "
    // 012345678901
    // ws "  " (0..2)
    // ident "my_ident" (2..10)
    // span should be 2..10

    assert_eq!(result.span, 2..10);
}

// -----------------------------------------------------------------------------
// A span is byte offsets, and where a person looks is a
// step away.
// -----------------------------------------------------------------------------

mod places {
    use winnow_grammar::grammar;
    use winnow_grammar::span::{line_column, SpanExt};
    use winnow_grammar::testing::WinnowTestExt;

    grammar! {
        grammar Located {
            pub second -> std::ops::Range<usize> = _a:raw_ident _b:raw_ident @ s -> { s }
        }
    }

    #[test]
    fn a_span_slices_the_source_and_names_a_place() {
        let src = "alpha\n  beta";
        let span = Located::parse_second().parse_test(src).assert_success();

        // What it points at, without the caller doing arithmetic.
        assert_eq!(span.text(src), "beta");

        // And where that is, for a person: line 2, column 3.
        assert_eq!(span.line_columns(src), ((2, 3), (2, 7)));
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        // `ü` is two bytes; the column a reader sees is the third character.
        let src = "füür x";
        assert_eq!(line_column(src, 6), (1, 5));
    }

    #[test]
    fn an_offset_past_the_end_is_clamped() {
        let src = "ab\n";
        assert_eq!(line_column(src, 999), (2, 1));
        assert_eq!((1..999).text(src), "b\n");
    }

    #[test]
    fn it_is_the_same_function_the_errors_use() {
        // One implementation, so a span and an error message can never
        // disagree about where something is.
        let src = "alpha\nbeta !";
        let rendered = Located::parse_second().parse_test(src).assert_failure();
        let (line, column) = line_column(src, src.find('!').unwrap());
        assert!(
            rendered.contains(&format!("line {line}, column {column}")),
            "{rendered}"
        );
    }
}

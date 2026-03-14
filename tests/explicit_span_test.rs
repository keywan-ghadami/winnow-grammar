use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::grammar;

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
        pub rule custom_node -> CustomNode @= a:ident -> { CustomNode::from_data(a.to_string(), _span) }
    }
}

#[test]
fn test_explicit_span_injection() {
    let input = "  my_ident  ";
    let input = LocatingSlice::new(input);
    let result = ExplicitSpanTest::parse_custom_node.parse(input).unwrap();

    assert_eq!(result.name, "my_ident");
    // "  my_ident  "
    // 012345678901
    // ws "  " (0..2)
    // ident "my_ident" (2..10)
    // span should be 2..10

    assert_eq!(result.span, 2..10);
}

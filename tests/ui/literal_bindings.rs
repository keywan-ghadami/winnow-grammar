use winnow::Parser;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;

grammar! {
    grammar LiteralBindings {
        pub rule literal_binding -> String =
            label:"literal" -> { label.to_string() }

        pub rule optional_literal_binding -> Option<String> =
            label:"literal"? -> { label.map(|s| s.to_string()) }
        
        pub rule literal_span_binding -> usize =
            "literal" @ span -> { span.end - span.start }
            
        pub rule literal_binding_with_span -> (String, usize) =
            label:"literal" @ span -> { (label.to_string(), span.end - span.start) }
    }
}

fn main() {
    let input = LocatingSlice::new("literal");
    let parsed = LiteralBindings::parse_literal_binding.parse(input).unwrap();
    assert_eq!(parsed, "literal");

    let input = LocatingSlice::new("literal");
    let parsed = LiteralBindings::parse_optional_literal_binding.parse(input).unwrap();
    assert_eq!(parsed, Some("literal".to_string()));

    let input = LocatingSlice::new("");
    let parsed = LiteralBindings::parse_optional_literal_binding.parse(input).unwrap();
    assert_eq!(parsed, None);

    let input = LocatingSlice::new("literal");
    let parsed = LiteralBindings::parse_literal_span_binding.parse(input).unwrap();
    assert_eq!(parsed, 7);

    let input = LocatingSlice::new("literal");
    let parsed = LiteralBindings::parse_literal_binding_with_span.parse(input).unwrap();
    assert_eq!(parsed, ("literal".to_string(), 7));
}

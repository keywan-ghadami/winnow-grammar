use winnow_grammar::test_case;

test_case! {
    line_ending_test,
    rule: test_line_ending,
    {
        // We override ws to do nothing so we can test whitespace sensitive parsers
        // Using "custom_ws" to avoid conflict if any, but rule ws -> () is the standard override.
        // We need to make sure we don't recurse infinitely if ws calls ws.
        // Empty string literal is a parser that consumes nothing and succeeds.
        WS = empty
        pub rule test_line_ending -> String =
            s:line_ending -> { s.to_string() }
    },
    [
        ("\n", val "\n".to_string()),
        ("\r\n", val "\r\n".to_string()),
        ("a", err "expected line ending; found unexpected token `a`")
    ]
}

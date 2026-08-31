use winnow_grammar::test_case;

test_case!(
    test_explicit_lex_repro,
    {
        pub main -> (String, String) = "start" lex("a" "b") -> { ("a".into(), "b".into()) }
    },
    [
        ("start ab", val ("a".into(), "b".into()))
    ]
);

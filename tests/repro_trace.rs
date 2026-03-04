use winnow_grammar::winnow_test_case;

winnow_test_case!(
    test_explicit_lex_repro,
    {
        pub rule main -> (String, String) = "start" lex("a" "b") -> { ("a".into(), "b".into()) }
    },
    [
        ("start ab", val ("a".into(), "b".into()))
    ]
);

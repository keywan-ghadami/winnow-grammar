use winnow_grammar::winnow_test_case;

winnow_test_case!(
    test_spaced_repro,
    rule: MAIN,
    {
        pub rule MAIN -> (String, String) = "a" spaced("b" "c") -> { ("b".into(), "c".into()) }
    },
    [
        ("ab c", val ("b".into(), "c".into())),
        ("a b c", err "expected `b`")
    ]
);

use winnow_grammar::test_case;

test_case!(
    test_syntactic_rule,
    {
        pub rule main -> (String, String) = "a" "b" -> { ("a".into(), "b".into()) }
    },
    [
        ("a b", val ("a".into(), "b".into())),
        ("ab", val ("a".into(), "b".into())),
        (" a   b ", val ("a".into(), "b".into()))
    ]
);

test_case!(
    test_lexical_rule,
    rule: MAIN,
    {
        pub rule MAIN -> (String, String) = "a" "b" -> { ("a".into(), "b".into()) }
    },
    [
        ("ab", val ("a".into(), "b".into())),
        ("a b", err "expected `b`") // In lexical mode, "a" is followed by "b" immediately. "a b" fails.
    ]
);

test_case!(
    test_explicit_lex,
    {
        pub rule main -> (String, String) = "start" lex("a" "b") -> { ("a".into(), "b".into()) }
    },
    [
        ("start ab", val ("a".into(), "b".into())),
        ("start a b", err "expected `b`")
    ]
);

test_case!(
    test_spaced_in_lex,
    rule: MAIN,
    {
        pub rule MAIN -> (String, String) = "a" spaced("b" "c") -> { ("b".into(), "c".into()) }
    },
    [
        ("ab c", val ("b".into(), "c".into())),
        ("abc", val ("b".into(), "c".into())),
        ("a b c", err "expected `b`")
    ]
);

test_case!(
    test_nested_scopes,
    {
        pub rule main -> String =
            "outer"
            lex(
                "inner"
                spaced("deep" "space")
                "end"
            )
            -> { "ok".into() }
    },
    [
        ("outer innerdeep spaceend", val "ok"),
        ("outer   innerdeep   spaceend", val "ok"),
        ("outer inner deep space end", err "expected `deep`"), // Space after "inner" fails in lex
        ("outer innerdeep space end", err "expected `end`"), // Space before "end" fails in lex
    ]
);

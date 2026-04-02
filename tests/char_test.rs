use winnow_grammar::test_case;

test_case! {
    char_test,
    rule: test_char,
    {
        pub rule test_char -> char =
            c:char -> { c }
    },
    [
        ("'a'", val 'a'),
        ("'\\n'", val '\n'),
        ("'\\\''", val '\''),
        ("'\\\\'", val '\\')
    ]
}

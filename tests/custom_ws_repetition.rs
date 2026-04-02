use winnow_grammar::test_case;

test_case! {
    custom_ws_repetition_test,
    rule: list,
    {
        // Override ws to match underscore OR nothing
        WS = ("_")?

        pub rule list -> Vec<u32> = l:u32+ -> { l }
    },
    [
        ("1_2_3", val vec![1, 2, 3])
    ]
}

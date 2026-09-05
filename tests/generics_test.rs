use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar Generics {
        // Form 1: the parameter is declared as a rule, its result type bound to T.
        list<T>(item: Rule<T>) -> Vec<T> =
            elements:item* -> { elements }
        pub main -> Vec<u32> = l:list<u32>(item=u32) -> { l }

        // Form 2: parameter without type, type parameter explicit at the call site.
        list_untyped<T>(item) -> Vec<T> = items:item* -> { items }
        pub explicit -> Vec<u32> = l:list_untyped<u32>(item=u32) -> { l }

        // Form 3: parameter without type, type parameter inferred from the argument -
        // the form from SYNTAX.md. Previously: "cannot find value `item`".
        pub inferred -> Vec<u32> = l:list_untyped(item=u32) -> { l }

        // Type parameter also in the action block: must be substituted too. The
        // block contains statements - previously that did not work anywhere
        // ("expected expression, found `let` statement").
        collected<T>(item) -> Vec<T> = items:item* -> { let mut v: Vec<T> = Vec::new(); v.extend(items); v }
        pub in_action_block -> Vec<u32> = l:collected(item=u32) -> { l }
    }
}

#[test]
fn test_generics() {
    Generics::parse_main()
        .parse_test("1 2 3")
        .assert_success_is(vec![1, 2, 3]);
}

#[test]
fn parser_parameter_without_type_explicit_generics() {
    Generics::parse_explicit()
        .parse_test("1 2 3")
        .assert_success_is(vec![1, 2, 3]);
}

#[test]
fn parser_parameter_without_type_inferred_generics() {
    Generics::parse_inferred()
        .parse_test("4 5")
        .assert_success_is(vec![4, 5]);
}

#[test]
fn type_parameter_in_action_block_is_substituted() {
    Generics::parse_in_action_block()
        .parse_test("7")
        .assert_success_is(vec![7]);
}

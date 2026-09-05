use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar Generics {
        // Form 1: the parameter is declared as a rule, its result type bound to T.
        list<T>(item: Rule<T>) -> Vec<T> =
            elements:item* -> { elements }
        pub main -> Vec<u32> = l:list<u32>(item=u32) -> { l }

        // Form 2: parameter without type, type parameter explicit at the call site.
        liste<T>(item) -> Vec<T> = items:item* -> { items }
        pub explizit -> Vec<u32> = l:liste<u32>(item=u32) -> { l }

        // Form 3: parameter without type, type parameter inferred from the argument -
        // the form from SYNTAX.md. Previously: "cannot find value `item`".
        pub abgeleitet -> Vec<u32> = l:liste(item=u32) -> { l }

        // Type parameter also in the action block: must be substituted too. The
        // block contains statements - previously that did not work anywhere
        // ("expected expression, found `let` statement").
        gesammelt<T>(item) -> Vec<T> = items:item* -> { let mut v: Vec<T> = Vec::new(); v.extend(items); v }
        pub im_aktionsblock -> Vec<u32> = l:gesammelt(item=u32) -> { l }
    }
}

#[test]
fn test_generics() {
    Generics::parse_main()
        .parse_test("1 2 3")
        .assert_success_is(vec![1, 2, 3]);
}

#[test]
fn parser_parameter_ohne_typ_explizite_generics() {
    Generics::parse_explizit()
        .parse_test("1 2 3")
        .assert_success_is(vec![1, 2, 3]);
}

#[test]
fn parser_parameter_ohne_typ_abgeleitete_generics() {
    Generics::parse_abgeleitet()
        .parse_test("4 5")
        .assert_success_is(vec![4, 5]);
}

#[test]
fn typparameter_im_aktionsblock_wird_ersetzt() {
    Generics::parse_im_aktionsblock()
        .parse_test("7")
        .assert_success_is(vec![7]);
}

use winnow::prelude::*;
use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar Generics {
        // Form 1: der Parameter ist als Regel deklariert, sein Ergebnistyp an T gebunden.
        list<T>(item: Rule<T>) -> Vec<T> =
            elements:item* -> { elements }
        pub main -> Vec<u32> = l:list<u32>(item=u32) -> { l }

        // Form 2: Parameter ohne Typ, Typparameter explizit am Aufruf.
        liste<T>(item) -> Vec<T> = items:item* -> { items }
        pub explizit -> Vec<u32> = l:liste<u32>(item=u32) -> { l }

        // Form 3: Parameter ohne Typ, Typparameter aus dem Argument abgeleitet -
        // die Form aus SYNTAX.md. Vorher: "cannot find value `item`".
        pub abgeleitet -> Vec<u32> = l:liste(item=u32) -> { l }

        // Typparameter auch im Aktionsblock: muss mit ersetzt werden. Der Block
        // enthaelt Anweisungen - das ging vorher an keiner Stelle
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

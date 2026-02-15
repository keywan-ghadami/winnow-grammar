use syn::parse::Parser;
use syn_grammar::grammar;
use syn_grammar::testing::Testable;

// --- Test Peek (Positive Lookahead) ---
#[test]
fn test_peek_basic() {
    grammar! {
        grammar peek_test {
            // matches "a" only if followed by "b", but "b" is not consumed by the rule itself (or is consumed later?)
            // "a" peek("b") "b" -> consumes "a" "b".
            // "a" peek("b") "c" -> fails.
            rule main -> String = "a" peek("b") next:ident -> { next.to_string() }
        }
    }

    // "a b" -> OK. "a" matches. peek("b") sees "b" (success). next:ident consumes "b".
    peek_test::parse_main
        .parse_str("a b")
        .test()
        .assert_success_is("b".to_string());

    // "a c" -> FAIL. peek("b") fails.
    peek_test::parse_main
        .parse_str("a c")
        .test()
        .assert_failure_contains("expected `b`");
}

#[test]
fn test_peek_binding() {
    grammar! {
        grammar peek_bind {
            // Bindings in peek are accessible?
            // peek(i:ident) -> i is bound.
            rule main -> String = peek(i:ident) "foo" -> { i.to_string() }
        }
    }

    // "foo" -> peek(i:ident) matches "foo". i="foo". then "foo" matches "foo".
    peek_bind::parse_main
        .parse_str("foo")
        .test()
        .assert_success_is("foo".to_string());
}

// --- Test Not (Negative Lookahead) ---
#[test]
fn test_not_basic() {
    grammar! {
        grammar not_test {
            // matches "a" only if NOT followed by "b".
            rule main -> () = "a" not("b") next:ident -> { () }
        }
    }

    // "a c" -> OK. not("b") sees "c" (success). next consumes "c".
    not_test::parse_main
        .parse_str("a c")
        .test()
        .assert_success();

    // "a b" -> FAIL. not("b") sees "b" (fail).
    not_test::parse_main
        .parse_str("a b")
        .test()
        .assert_failure_contains("unexpected match");
}

#[test]
fn test_not_complex() {
    grammar! {
        grammar not_complex {
            // Ensure `not` works with rules
            rule main -> () = not(bad) any:ident -> { () }
            rule bad -> () = "bad" -> { () }
        }
    }

    not_complex::parse_main
        .parse_str("good")
        .test()
        .assert_success();
    not_complex::parse_main
        .parse_str("bad")
        .test()
        .assert_failure_contains("unexpected match");
}

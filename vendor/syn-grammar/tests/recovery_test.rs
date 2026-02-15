use syn::parse::Parser;
use syn_grammar::grammar;
use syn_grammar::testing::Testable;

#[derive(Debug, PartialEq)]
pub struct Stmt;

#[test]
fn test_failure_recovery() {
    grammar! {
        grammar recovery_simple {
            use super::Stmt;

            // If `parse_stmt` fails, skip until `;`.
            // Returns Option<Stmt>
            pub rule block -> Vec<Option<Stmt>> =
                { stmts:stmt_recovered* } -> { stmts }

            rule stmt_recovered -> Option<Stmt> =
                s:recover(parse_stmt, ";") ";" -> { s }

            rule parse_stmt -> Stmt =
                "let" "x" -> { Stmt }
        }
    }

    // "let x;" -> Success
    recovery_simple::parse_block
        .parse_str("{ let x; }")
        .test()
        .assert_success_is(vec![Some(Stmt)]);

    // "let y;" -> Fail inside stmt, recover at ;
    recovery_simple::parse_block
        .parse_str("{ let y; }")
        .test()
        .assert_success_is(vec![None]);

    // "garbage;" -> Fail inside stmt, recover at ;
    recovery_simple::parse_block
        .parse_str("{ garbage; }")
        .test()
        .assert_success_is(vec![None]);

    // "let x; garbage; let x;" -> Success, Fail, Success
    recovery_simple::parse_block
        .parse_str("{ let x; garbage; let x; }")
        .test()
        .assert_success_is(vec![Some(Stmt), None, Some(Stmt)]);
}

#[test]
fn test_recovery_complex_sync() {
    grammar! {
        grammar recovery_complex {
            pub rule main -> Vec<Option<i32>> =
                items:item* -> { items }

            rule item -> Option<i32> =
                // Recover until "next" keyword
                v:recover(val, "next") "next" -> { v }

            rule val -> i32 =
                "val" i:i32 -> { i }
        }
    }

    // "val 42 next" -> OK
    // "val broken next" -> Fail val, skip to next, return None
    recovery_complex::parse_main
        .parse_str("val 42 next val broken next")
        .test()
        .assert_success_is(vec![Some(42), None]);
}

#[test]
fn test_attempt_recover_behavior() {
    // This tests that `recover` uses `attempt_recover` which does NOT backtrack on success,
    // but DOES backtrack on failure to allow re-synchronization.
    grammar! {
        grammar recover_check {
            pub rule main -> String =
                // If this fails, we want to consume tokens until "end"
                s:recover(start_rule, "end") "end" -> {
                    s.unwrap_or_else(|| "recovered".to_string())
                }

            rule start_rule -> String =
                "start" i:i32 -> { i.to_string() }
        }
    }

    // Success case
    recover_check::parse_main
        .parse_str("start 123 end")
        .test()
        .assert_success_is("123".to_string());

    // Failure case: "start" matches, "broken" fails integer parse.
    // recover should catch the error and skip until "end"
    recover_check::parse_main
        .parse_str("start broken end")
        .test()
        .assert_success_is("recovered".to_string());
}

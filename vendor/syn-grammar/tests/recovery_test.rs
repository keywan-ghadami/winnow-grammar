use syn::parse::Parser;
use syn_grammar::grammar;
use syn_grammar::testing::Testable;

#[test]
fn test_failure_recovery() {
    grammar! {
        grammar recovery {
            // We parse a list of statements.
            // If a statement fails, we recover at the semicolon.
            rule main -> Vec<Option<String>> =
                stmts:stmt_wrapper* -> { stmts }

            // recover() skips until ";", but does not consume it.
            // We must consume ";" explicitly.
            rule stmt_wrapper -> Option<String> =
                s:recover(stmt, ";") ";" -> { s }

            rule stmt -> String =
                "let" name:ident -> { format!("let {}", name) }
        }
    }

    // Input:
    // 1. "let a;" -> Valid
    // 2. "let 123;" -> Invalid (123 is not ident), should recover at ;
    // 3. "let b;" -> Valid
    let input = "let a; let 123; let b;";

    let res = recovery::parse_main
        .parse_str(input)
        .test()
        .assert_success();

    assert_eq!(res.len(), 3);
    assert_eq!(res[0], Some("let a".to_string()));
    assert_eq!(res[1], None); // Recovered!
    assert_eq!(res[2], Some("let b".to_string()));
}

#[test]
fn test_recovery_complex_sync() {
    grammar! {
        grammar recovery_complex {
            rule main -> Vec<Option<i32>> =
                items:item* -> { items }

            // Recover until we see "end"
            // Note: The sync pattern "end" is NOT consumed by recover logic.
            // We must consume it explicitly.
            rule item -> Option<i32> =
                "group" i:recover(inner, "end") "end" -> { i }

            rule inner -> i32 =
                "val" i:integer -> { i }
        }
    }

    // 1. group val 10 end -> OK
    // 2. group val x end  -> Error (x is not int), skip to 'end', return None
    // 3. group val 20 end -> OK
    let input = "group val 10 end group val x end group val 20 end";

    let res = recovery_complex::parse_main
        .parse_str(input)
        .test()
        .assert_success();

    assert_eq!(res, vec![Some(10), None, Some(20)]);
}

#[test]
fn test_attempt_recover_behavior() {
    grammar! {
        grammar recover_check {
            // recover(inner, "end") tries inner.
            // If inner fails, it returns None and the generated code skips until "end".
            // We must then consume "end" explicitly.
            rule main -> String =
                res:recover(inner, "end") "end" -> {
                    res.unwrap_or_else(|| "recovered".to_string())
                }

            rule inner -> String =
                "start" i:integer -> { i.to_string() }
        }
    }

    // 1. Success path
    let res = recover_check::parse_main.parse_str("start 42 end");
    assert_eq!(res.unwrap(), "42");

    // 2. Failure path (Recovery)
    // "start" matches, "broken" fails integer parse.
    // recover catches error, skips "broken".
    // stops at "end".
    // main consumes "end".
    let res = recover_check::parse_main.parse_str("start broken end");
    assert_eq!(res.unwrap(), "recovered");
}

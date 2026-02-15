use syn::parse::Parser;
use syn_grammar::grammar;
use syn_grammar::testing::Testable;

// --- Test Action Block Statements ---
#[test]
fn test_action_block_statements() {
    grammar! {
        grammar action_stmt {
            pub rule main -> i32 = "x" -> {
                let a = 10;
                let b = 32;
                a + b
            }
        }
    }

    action_stmt::parse_main
        .parse_str("x")
        .test()
        .assert_success_is(42);
}

// --- Test Built-ins ---
#[test]
fn test_builtins() {
    grammar! {
        grammar builtins_test {
            pub rule test_int -> i32 = i:i32 -> { i }
            pub rule test_str -> String = s:string -> { s.value }
        }
    }

    builtins_test::parse_test_int
        .parse_str("123")
        .test()
        .assert_success_is(123);

    builtins_test::parse_test_str
        .parse_str("\"hello\"")
        .test()
        .assert_success_is("hello".to_string());
}

// --- Test Repetition ---
#[test]
fn test_repetition() {
    grammar! {
        grammar repetition_test {
            pub rule star -> Vec<i32> = ( items:i32* ) -> { items }
            pub rule plus -> Vec<i32> = ( items:i32+ ) -> { items }
            pub rule opt -> Option<i32> = ( item:i32? ) -> { item }
        }
    }

    repetition_test::parse_star
        .parse_str("1 2 3")
        .test()
        .assert_success_is(vec![1, 2, 3]);

    repetition_test::parse_star
        .parse_str("")
        .test()
        .assert_success_is(vec![]);

    repetition_test::parse_plus
        .parse_str("1 2")
        .test()
        .assert_success_is(vec![1, 2]);

    repetition_test::parse_plus
        .parse_str("")
        .test()
        .assert_failure_contains("expected integer");

    repetition_test::parse_opt
        .parse_str("42")
        .test()
        .assert_success_is(Some(42));

    repetition_test::parse_opt
        .parse_str("")
        .test()
        .assert_success_is(None);
}

// --- Test Nested Repetition & Complex Types ---
#[test]
fn test_nested_repetition_complex() {
    grammar! {
        grammar complex_rep {
            pub rule main -> Vec<Vec<i32>> =
                // ( ( integer )* )*
                // We use delimiters to keep structure simple for the parser
                ( groups:group* ) -> { groups }

            rule group -> Vec<i32> =
                paren(items:i32*) -> { items }
        }
    }

    // "(1 2) (3)" -> [[1, 2], [3]]
    complex_rep::parse_main
        .parse_str("(1 2) (3)")
        .test()
        .assert_success_is(vec![vec![1, 2], vec![3]]);
}

// --- Test Cut Operator ---
#[test]
fn test_cut_operator() {
    grammar! {
        grammar cut_test {
            pub rule main -> i32 =
                // If "let" is seen, commit to this arm.
                // If "let" is followed by something other than integer, fail immediately.
                "let" => i:i32 -> { i }
              | i:i32 -> { i }
        }
    }

    // "let 42" -> 42
    cut_test::parse_main
        .parse_str("let 42")
        .test()
        .assert_success_is(42);

    // "42" -> 42 (second arm)
    cut_test::parse_main
        .parse_str("42")
        .test()
        .assert_success_is(42);

    // "let bad" -> Error (expected integer), does NOT backtrack to second arm
    cut_test::parse_main
        .parse_str("let bad")
        .test()
        .assert_failure_contains("expected integer");
}

// --- Test Left Recursion ---
#[test]
fn test_left_recursion() {
    grammar! {
        grammar expr_test {
            pub rule expr -> i32 =
                l:expr "+" r:term -> { l + r }
              | l:expr "-" r:term -> { l - r }
              | t:term -> { t }

            rule term -> i32 =
                l:term "*" r:factor -> { l * r }
              | f:factor -> { f }

            rule factor -> i32 =
                i:i32 -> { i }
              | paren(e:expr) -> { e }
        }
    }

    // 1 + 2 * 3 -> 7
    expr_test::parse_expr
        .parse_str("1 + 2 * 3")
        .test()
        .assert_success_is(7);

    // (1 + 2) * 3 -> 9
    expr_test::parse_expr
        .parse_str("(1 + 2) * 3")
        .test()
        .assert_success_is(9);

    // 1 - 2 - 3 -> (1 - 2) - 3 = -4 (Left associative)
    expr_test::parse_expr
        .parse_str("1 - 2 - 3")
        .test()
        .assert_success_is(-4);
}

// --- Test Keywords vs Idents ---
#[test]
fn test_keywords_vs_idents() {
    grammar! {
        grammar kw_test {
            pub rule main -> String =
                "fn" name:ident -> { name.to_string() }
        }
    }

    kw_test::parse_main
        .parse_str("fn foo")
        .test()
        .assert_success_is("foo".to_string());

    // "fn fn" -> Error (expected ident, got keyword fn - syn behavior)
    // Actually syn::parse_ident fails on keywords unless raw identifiers are used.
    kw_test::parse_main
        .parse_str("fn fn")
        .test()
        .assert_failure_contains("expected identifier");
}

// --- Test Basic Sequence ---
#[test]
fn test_basic_sequence() {
    grammar! {
        grammar seq_test {
            pub rule main -> (i32, i32) = a:i32 b:i32 -> { (a, b) }
        }
    }

    seq_test::parse_main
        .parse_str("10 20")
        .test()
        .assert_success_is((10, 20));
}

// --- Test Epsilon Alternative ---
#[test]
fn test_epsilon_alternative() {
    grammar! {
        grammar epsilon {
            pub rule main -> Option<i32> =
                i:i32 -> { Some(i) }
              | -> { None }
        }
    }

    epsilon::parse_main
        .parse_str("42")
        .test()
        .assert_success_is(Some(42));
    epsilon::parse_main
        .parse_str("")
        .test()
        .assert_success_is(None);
}

// --- Test Rule Arguments ---
#[test]
fn test_rule_arguments() {
    grammar! {
        grammar args {
            pub rule main -> i32 = "start" v:value(10) -> { v }
            rule value(offset: i32) -> i32 = i:i32 -> { i + offset }
        }
    }

    args::parse_main
        .parse_str("start 5")
        .test()
        .assert_success_is(15);
}

// --- Test Multiple Arguments ---
#[test]
fn test_multiple_arguments() {
    grammar! {
        grammar multi_args {
            pub rule main -> i32 = "calc" v:calc(2, 3) -> { v }
            rule calc(mult: i32, base: i32) -> i32 = i:i32 -> { base + (i * mult) }
        }
    }

    // 10 * 2 + 3 = 23
    multi_args::parse_main
        .parse_str("calc 10")
        .test()
        .assert_success_is(23);
}

// --- Test Complex Return Types ---
// For simplicity in this test, we won't return Result<_,_> because syn::Error doesn't impl PartialEq easily.
// We'll wrap in Option.
#[test]
fn test_complex_return_types() {
    grammar! {
        grammar types {
            pub rule main -> Vec<Option<i32>> =
                items:item* -> { items }

            rule item -> Option<i32> =
                i:i32 -> { Some(i) }
              | s:string -> { None }
        }
    }

    types::parse_main
        .parse_str("10 \"skip\" 20")
        .test()
        .assert_success_is(vec![Some(10), None, Some(20)]);
}

// --- Test Cut in Repetition ---
#[test]
fn test_cut_in_repetition() {
    grammar! {
        grammar cut_rep {
            pub rule main -> Vec<i32> =
                // Once we see "item", we commit to `integer`.
                // If `integer` is missing after "item", we error and abort the loop (and the rule).
                ( "item" => i:i32 )* -> { i }
        }
    }

    cut_rep::parse_main
        .parse_str("item 1 item 2")
        .test()
        .assert_success_is(vec![1, 2]);

    // "item 1 item" -> Fail (expected integer after second item)
    cut_rep::parse_main
        .parse_str("item 1 item")
        .test()
        .assert_failure_contains("expected integer");
}

// --- Test Backtracking Priority ---
#[test]
fn test_backtracking_priority() {
    grammar! {
        grammar priority {
            pub rule main -> i32 =
                // Longest match first
                "a" "b" "c" -> { 3 }
              | "a" "b"     -> { 2 }
              | "a"         -> { 1 }
        }
    }

    priority::parse_main
        .parse_str("a b c")
        .test()
        .assert_success_is(3);
    priority::parse_main
        .parse_str("a b")
        .test()
        .assert_success_is(2);
    priority::parse_main
        .parse_str("a")
        .test()
        .assert_success_is(1);
}

// --- Test Use Statements ---
#[test]
fn test_use_statements() {
    grammar! {
        grammar use_stmt {
            use std::collections::HashMap;

            pub rule main -> HashMap<String, i32> =
                "map" -> { HashMap::new() }
        }
    }

    use_stmt::parse_main
        .parse_str("map")
        .test()
        .assert_success();
}

// --- Test Left Recursion Field Access ---
#[test]
fn test_left_recursion_field_access() {
    grammar! {
        grammar field_access {
            pub rule expr -> String =
                l:expr "." r:ident -> { format!("{}.{}", l, r) }
              | i:ident -> { i.to_string() }
        }
    }

    field_access::parse_expr
        .parse_str("a.b.c")
        .test()
        .assert_success_is("a.b.c".to_string());
}

// --- Test Multi-token Literals ---
#[test]
fn test_multi_token_literals() {
    grammar! {
        grammar multi_tok {
            pub rule main -> bool =
                "?." -> { true }
        }
    }

    // Matches strict adjacency
    multi_tok::parse_main
        .parse_str("?.")
        .test()
        .assert_success_is(true);
    // Fails on space
    multi_tok::parse_main
        .parse_str("? .")
        .test()
        .assert_failure_contains("expected '?.', found space between tokens");
}

// --- Test Extended Literals ---
#[test]
fn test_extended_literals() {
    grammar! {
        grammar ext_lit {
            pub rule attr -> () = "@detached" -> { () }
        }
    }

    ext_lit::parse_attr
        .parse_str("@detached")
        .test()
        .assert_success();
    ext_lit::parse_attr
        .parse_str("@ detached")
        .test()
        .assert_failure_contains("expected '@detached', found space between tokens");
}

// --- Test Attributes on Rules ---
#[test]
fn test_attributes_on_rules() {
    grammar! {
        grammar attr_rules {
            /// Doc comment
            #[allow(dead_code)]
            pub rule main -> () = "a" -> { () }
        }
    }
    attr_rules::parse_main
        .parse_str("a")
        .test()
        .assert_success();
}

// --- Test Plus Operator Validation ---
#[test]
fn test_plus_operator_validation() {
    grammar! {
        grammar plus_val {
            pub rule list -> Vec<i32> = i:i32+ -> { i }
        }
    }
    plus_val::parse_list
        .parse_str("1 2")
        .test()
        .assert_success_is(vec![1, 2]);
    plus_val::parse_list
        .parse_str("")
        .test()
        .assert_failure_contains("expected integer");
}

// --- Test Math Expression (Integration) ---
#[test]
fn test_math_expression() {
    grammar! {
        grammar math {
            pub rule expr -> i32 =
                l:expr "+" r:term -> { l + r }
              | l:expr "-" r:term -> { l - r }
              | t:term            -> { t }

            rule term -> i32 =
                l:term "*" r:factor -> { l * r }
              | l:term "/" r:factor -> { l / r }
              | f:factor            -> { f }

            rule factor -> i32 =
                i:i32               -> { i }
              | paren(e:expr)      -> { e }
        }
    }

    math::parse_expr
        .parse_str("10 + 2 * 3")
        .test()
        .assert_success_is(16);
    math::parse_expr
        .parse_str("(10 + 2) * 3")
        .test()
        .assert_success_is(36);
}

// --- Test Rust Types and Blocks ---
#[test]
fn test_rust_types_and_blocks() {
    grammar! {
        grammar rust_features {
            pub rule type_parser -> syn::Type = t:rust_type -> { t }
            pub rule block_parser -> syn::Block = b:rust_block -> { b }
        }
    }

    rust_features::parse_type_parser
        .parse_str("Vec<i32>")
        .test()
        .assert_success();

    rust_features::parse_block_parser
        .parse_str("{ let x = 1; }")
        .test()
        .assert_success();
}

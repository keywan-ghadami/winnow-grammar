use syn::parse::Parser;
use syn_grammar::grammar;
use syn_grammar::testing::Testable;

// --- Test 1: Basic Sequence ---
#[test]
fn test_basic_sequence() {
    grammar! {
        grammar basic {
            rule main -> String = "hello" "world" -> { "Success".to_string() }
        }
    }

    // NEW: .assert_success_is(...)
    basic::parse_main
        .parse_str("hello world")
        .test()
        .assert_success_is("Success");

    // NEW: .assert_failure_contains(...)
    basic::parse_main
        .parse_str("hello universe")
        .test()
        .assert_failure_contains("expected `world`");
}

// --- Test 2: Backtracking & Priority ---
#[test]
fn test_backtracking_priority() {
    grammar! {
        grammar backtrack {
            rule main -> String =
                "A" "B" -> { "Path AB".to_string() }
              | "A"     -> { "Path A".to_string() }
        }
    }

    backtrack::parse_main
        .parse_str("A B")
        .test()
        .assert_success_is("Path AB");

    backtrack::parse_main
        .parse_str("A")
        .test()
        .assert_success_is("Path A");
}

// --- Test 3: Complex Groups & Optionality ---
#[test]
fn test_complex_groups() {
    grammar! {
        grammar complex {
            rule main -> String = ("A" "B")? "C" -> { "OK".to_string() }
        }
    }

    complex::parse_main
        .parse_str("A B C")
        .test()
        .assert_success();
    complex::parse_main.parse_str("C").test().assert_success();

    // Here we expect it to fail because "B" is missing
    complex::parse_main.parse_str("A C").test().assert_failure();
}

// --- Test 4: Mathematical Expressions ---
#[test]
fn test_math_expression() {
    grammar! {
        grammar math {
            rule main -> i32 = e:expr -> { e }

            rule expr -> i32 =
                t:term "+" e:expr -> { t + e }
              | t:term            -> { t }

            rule term -> i32 =
                f:factor "*" t:term -> { f * t }
              | f:factor            -> { f }

            rule factor -> i32 =
                paren(e:expr)  -> { e }
              | i:integer      -> { i }
        }
    }

    math::parse_main
        .parse_str("2 + 3 * 4")
        .test()
        .assert_success_is(14);

    math::parse_main
        .parse_str("(2 + 3) * 4")
        .test()
        .assert_success_is(20);
}

// --- Test 5: Repetitions & Token Brackets ---
#[test]
fn test_repetition() {
    grammar! {
        grammar repeat {
            rule main -> usize = [ content:elems ] -> { content }

            rule elems -> usize =
                first:elem rest:elem* -> { 1 + rest.len() }

            rule elem -> () = "x" ","? -> { () }
        }
    }

    repeat::parse_main
        .parse_str("[ x ]")
        .test()
        .assert_success_is(1);
    repeat::parse_main
        .parse_str("[ x, x, x ]")
        .test()
        .assert_success_is(3);
    repeat::parse_main.parse_str("[ ]").test().assert_failure();

    // Case: Missing closing bracket.
    repeat::parse_main
        .parse_str("[ x, x")
        .test()
        .assert_failure();
}

// --- Test 6: Built-ins ---
#[test]
fn test_builtins() {
    grammar! {
        grammar builtins {
            rule main -> (String, String) =
                k:ident "=" v:string -> { (k.to_string(), v) }
        }
    }

    builtins::parse_main
        .parse_str("config_key = \"some_value\"")
        .test()
        .assert_success_is(("config_key".to_string(), "some_value".to_string()));
}

// --- Test 7: Cut Operator (Syntax Check) ---
#[test]
fn test_cut_operator() {
    grammar! {
        grammar cut_test {
            // Scenario:
            // We want to distinguish a keyword "let" from an identifier "let".
            // If we match "let" literal, we CUT (=>). If the following pattern fails,
            // we should NOT backtrack to parse it as an identifier.
            rule main -> String =
                "let" => "mut" -> { "Variable Declaration".to_string() }
              | "let"          -> { "Identifier(let)".to_string() }
        }
    }

    // 1. Happy Path: Matches "let" then "mut"
    cut_test::parse_main
        .parse_str("let mut")
        .test()
        .assert_success_is("Variable Declaration");

    // 2. Edge Case: "let" followed by something else.
    //
    // Since the Cut operator is implemented, matching "let" commits to the first variant.
    // The parser will NOT backtrack to the second variant ("Identifier(let)").
    // Instead, it will fail because "mut" is expected but not found.
    cut_test::parse_main
        .parse_str("let")
        .test()
        .assert_failure_contains("expected `mut`");
}

// --- Test 8: Left Recursion (Operator Precedence) ---
#[test]
fn test_left_recursion() {
    grammar! {
        grammar left_rec {
            // Standard left-recursive definition for subtraction.
            // Parses "1 - 2 - 3" as "(1 - 2) - 3" = -4.
            // If it were right-recursive (or simple recursive descent without handling),
            // it might stack overflow or parse as "1 - (2 - 3)" = 2.
            pub rule expr -> i32 =
                l:expr "-" r:integer -> { l - r }
              | i:integer            -> { i }
        }
    }

    // 1. Simple
    left_rec::parse_expr
        .parse_str("10 - 2")
        .test()
        .assert_success_is(8);

    // 2. Associativity check: 10 - 2 - 3 => (10 - 2) - 3 = 5
    // (Right associative would be 10 - (2 - 3) = 10 - (-1) = 11)
    left_rec::parse_expr
        .parse_str("10 - 2 - 3")
        .test()
        .assert_success_is(5);
}

// --- Test 9: Left Recursion (Field Access) ---
#[test]
fn test_left_recursion_field_access() {
    grammar! {
        grammar field_access {
            pub rule expr -> String =
                e:expr "." i:ident -> { format!("({}).{}", e, i) }
              | i:ident            -> { i.to_string() }
        }
    }

    // a.b.c -> (a.b).c
    // With action format!("({}).{}", e, i):
    // 1. a -> "a"
    // 2. a.b -> "(a).b"
    // 3. (a).b.c -> "((a).b).c"
    field_access::parse_expr
        .parse_str("a.b.c")
        .test()
        .assert_success_is("((a).b).c".to_string());
}

// --- Test 10: Inheritance ---
// We define the grammars at the module level because the macro generates
// `use super::base::*;`, which requires `base` to be a sibling module.
grammar! {
    grammar base {
        pub rule num -> i32 = i:integer -> { i }
    }
}

grammar! {
    grammar derived : base {
        rule main -> i32 =
            "add" a:num b:num -> { a + b }
    }
}

#[test]
fn test_inheritance() {
    derived::parse_main
        .parse_str("add 10 20")
        .test()
        .assert_success_is(30);
}

// --- Test 11: Rust Types & Blocks ---
#[test]
fn test_rust_types_and_blocks() {
    grammar! {
        grammar rust_syntax {
            // Parses a type like "Vec<i32>"
            // We return a String debug representation to avoid complex AST assertions
            pub rule parse_type -> String =
                t:rust_type -> { format!("{:?}", t) }

            // Parses a block like "{ let x = 1; }"
            pub rule parse_block -> usize =
                b:rust_block -> { b.stmts.len() }
        }
    }

    // Test Type Parsing
    // We just assert success here to ensure the built-in parser consumes the input correctly.
    rust_syntax::parse_parse_type
        .parse_str("Vec<i32>")
        .test()
        .assert_success();

    // Test Block Parsing
    rust_syntax::parse_parse_block
        .parse_str("{ let x = 1; let y = 2; }")
        .test()
        .assert_success_is(2);
}

// --- Test 12: Keywords vs Identifiers ---
#[test]
fn test_keywords_vs_idents() {
    grammar! {
        grammar kw_test {
            // "fn" is a Rust keyword, "custom" is a custom keyword defined by usage literal
            rule main -> String =
                "fn" name:ident "custom" -> { name.to_string() }
        }
    }

    // Happy path
    kw_test::parse_main
        .parse_str("fn my_func custom")
        .test()
        .assert_success_is("my_func".to_string());

    // Fail: "custom" is expected, but found "other"
    kw_test::parse_main
        .parse_str("fn my_func other")
        .test()
        .assert_failure();

    // Fail: "fn" is expected
    kw_test::parse_main
        .parse_str("func my_func custom")
        .test()
        .assert_failure();
}

// --- Test 13: Missing Syntax Features (Plus, Braced, LitStr) ---
#[test]
fn test_missing_syntax_features() {
    grammar! {
        grammar missing {
            // Tests:
            // 1. braced { ... } (Note: 'braced' keyword is not needed/supported, just use { ... })
            // 2. + Operator (at least one element)
            // 3. Binding to a list (ids:ident+) -> Vec<Ident>
            rule main -> Vec<String> =
                { ids:ident+ } -> {
                    ids.iter().map(|id| id.to_string()).collect()
                }

            // Tests: lit_str (returns syn::LitStr, not String)
            pub rule raw_lit -> String =
                l:lit_str -> { l.value() }
        }
    }

    // 1. Happy Path for Braced + Plus
    missing::parse_main
        .parse_str("{ a b c }")
        .test()
        .assert_success_is(vec!["a".to_string(), "b".to_string(), "c".to_string()]);

    // 2. Fail Path for Plus (Empty list in Braces)
    // Since '+' expects at least one, "{ }" must fail.
    missing::parse_main.parse_str("{ }").test().assert_failure();

    // 3. Test for lit_str
    missing::parse_raw_lit
        .parse_str("\"raw content\"")
        .test()
        .assert_success_is("raw content".to_string());
}

// --- Test 14: Plus Operator Validation (Sad Path) ---
#[test]
fn test_plus_operator_validation() {
    grammar! {
        grammar plus_check {
            rule main -> Vec<String> =
                ids:ident+ -> { ids.iter().map(|i| i.to_string()).collect() }
        }
    }

    // Success: 1 or more
    plus_check::parse_main
        .parse_str("a")
        .test()
        .assert_success();
    plus_check::parse_main
        .parse_str("a b")
        .test()
        .assert_success();

    // Failure: 0 elements
    // This ensures + does not act like *
    plus_check::parse_main.parse_str("").test().assert_failure();
}

// --- Test 15: Nested Groups and Alternatives ---
#[test]
fn test_nested_groups_and_alts() {
    grammar! {
        grammar nested {
            // Pattern: ( "A" | ("B" "C") )*
            rule main -> usize =
                ( "A" | ("B" "C") )* -> { 0 }
        }
    }

    nested::parse_main
        .parse_str("A B C A")
        .test()
        .assert_success();
    // "B" must be followed by "C"
    nested::parse_main.parse_str("A B").test().assert_failure();
}

// --- Test 16: Cut in Repetition ---
#[test]
fn test_cut_in_repetition() {
    grammar! {
        grammar cut_loop {
            rule main -> usize =
                entries:entry* -> { entries.len() }

            rule entry -> () =
                "key" => "=" "val" ";" -> { () }
              | "other" ";"            -> { () }
        }
    }

    // Happy Path
    cut_loop::parse_main
        .parse_str("key = val ; other ;")
        .test()
        .assert_success_is(2);

    // Fail Path: "key" without "="
    // With Cut, it must fail with "expected `=`" immediately, not backtrack.
    cut_loop::parse_main
        .parse_str("key val ;")
        .test()
        .assert_failure_contains("expected `=`");
}

// --- Test 17: Complex Return Types ---
#[test]
fn test_complex_return_types() {
    grammar! {
        grammar types {
            // Tests handling of generics in return types
            rule main -> Vec<Option<i32>> =
                "null" -> { vec![None] }
        }
    }
    types::parse_main.parse_str("null").test().assert_success();
}

// --- Test 18: Empty (Epsilon) Alternatives ---
#[test]
fn test_epsilon_alternative() {
    grammar! {
        grammar epsilon {
            rule main -> String =
                "foo" -> { "foo".to_string() }
              |       -> { "empty".to_string() } // Empty alternative matches nothing (epsilon)
        }
    }

    epsilon::parse_main
        .parse_str("foo")
        .test()
        .assert_success_is("foo".to_string());
    epsilon::parse_main
        .parse_str("")
        .test()
        .assert_success_is("empty".to_string());
}

// --- Test 19: Inheritance Shadowing ---
grammar! {
    grammar base_shadow {
        pub rule value -> i32 = "one" -> { 1 }
    }
}

grammar! {
    grammar derived_shadow : base_shadow {
        // We override 'value' from base
        rule value -> i32 = "two" -> { 2 }

        rule main -> i32 = v:value -> { v }
    }
}

#[test]
fn test_inheritance_shadowing() {
    // Should use the local definition "two", not the imported "one"
    derived_shadow::parse_main
        .parse_str("two")
        .test()
        .assert_success_is(2);
}

// --- Test 20: Rule Arguments ---
#[test]
fn test_rule_arguments() {
    grammar! {
        grammar args_test {
            // Rule taking an argument from the caller
            rule main -> i32 =
                "start" v:value(10) -> { v }

            // Definition with parameters
            rule value(offset: i32) -> i32 =
                i:integer -> { i + offset }
        }
    }

    // "start 5" -> calls value(10) -> parses "5" -> returns 5 + 10 = 15
    args_test::parse_main
        .parse_str("start 5")
        .test()
        .assert_success_is(15);
}

// --- Test 21: Multiple Arguments ---
#[test]
fn test_multiple_arguments() {
    grammar! {
        grammar multi_args {
            rule main -> i32 =
                "calc" res:calc(10, 5) -> { res }

            // Tests comma separation in parameters
            rule calc(base: i32, mult: i32) -> i32 =
                i:integer -> { base + (i * mult) }
        }
    }

    // 10 + (2 * 5) = 20
    multi_args::parse_main
        .parse_str("calc 2")
        .test()
        .assert_success_is(20);
}

// --- Test 22: Complex Nested Repetition ---
#[test]
fn test_nested_repetition_complex() {
    grammar! {
        grammar nested_rep {
            // Pattern: ( "group" ( "item" )* ";" )*
            // Tests nested Vec generation and scopes
            rule main -> usize =
                groups:group* -> { groups.iter().sum() }

            rule group -> usize =
                "group" items:item* ";" -> { items.len() }

            rule item -> () = "item" -> { () }
        }
    }

    // 2 groups:
    // 1. group: 2 items
    // 2. group: 1 item
    // Total: 3
    let input = "group item item ; group item ;";
    nested_rep::parse_main
        .parse_str(input)
        .test()
        .assert_success_is(3);
}

// --- Test 23: Extended Literals ---
#[test]
fn test_extended_literals() {
    grammar! {
        grammar extended_lits {
            rule main -> (syn::LitInt, syn::LitChar, syn::LitBool, syn::LitFloat) =
                i:lit_int c:lit_char b:lit_bool f:lit_float -> { (i, c, b, f) }

            pub rule spanned -> ((i32, proc_macro2::Span), (String, proc_macro2::Span)) =
                i:lit_int @ is  s:lit_str @ ss -> {
                    ((i.base10_parse().unwrap(), is), (s.value(), ss))
                }
        }
    }

    // Test syn types
    let res = extended_lits::parse_main
        .parse_str("42 'c' true 3.123456")
        .unwrap();
    assert_eq!(res.0.base10_parse::<i32>().unwrap(), 42);
    assert_eq!(res.1.value(), 'c');
    assert!(res.2.value);

    let f_val = res.3.base10_parse::<f64>().unwrap();
    assert!(f_val > 3.123455 && f_val < 3.123457);

    // Test spanned
    let res = extended_lits::parse_spanned
        .parse_str("100 \"text\"")
        .unwrap();
    assert_eq!(res.0 .0, 100);
    assert_eq!(res.1 .0, "text");
}

// --- Test 24: Attributes on Rules ---
#[test]
fn test_attributes_on_rules() {
    grammar! {
        grammar attrs {
            // Test doc comments and standard attributes
            /// Parses an identifier
            #[allow(missing_docs)]
            pub rule main -> String = i:ident -> { i.to_string() }

            // Test conditional compilation (should be included in test profile)
            #[cfg(test)]
            pub rule test_only -> String = "test" -> { "present".to_string() }

            // Test allowing lints
            #[allow(non_snake_case)]
            pub rule CamelCase -> () = "camel" -> { () }

            // Test inline attribute for source verification
            #[inline]
            pub rule inline_rule -> () = "inline" -> { () }
        }
    }

    // 1. Verify standard attributes (doc comment, allow) are accepted.
    // If #[allow(missing_docs)] didn't work, we might get a warning (if denied).
    attrs::parse_main
        .parse_str("my_id")
        .test()
        .assert_success_is("my_id".to_string());

    // 2. Verify #[cfg(test)] works.
    // If the attribute wasn't applied, this function would exist in non-test builds too (hard to prove here),
    // but if the attribute parsing was broken, it might not exist or cause a syntax error.
    // The fact that it compiles and runs confirms the attribute was passed through to the generated function.
    attrs::parse_test_only
        .parse_str("test")
        .test()
        .assert_success_is("present".to_string());

    // 3. Verify #[allow(non_snake_case)] works.
    // If the attribute wasn't applied, `cargo clippy` would have failed/warned on this line.
    attrs::parse_CamelCase
        .parse_str("camel")
        .test()
        .assert_success();

    // 4. Verify #[inline] is present in the generated source.
    let src = attrs::GENERATED_SOURCE;
    let normalized: String = src.chars().filter(|c| !c.is_whitespace()).collect();

    // Use the testing framework to assert content
    let res: syn::Result<String> = Ok(normalized);
    res.test()
        .with_context("Checking generated source for #[inline] attribute")
        .assert_success_contains("#[inline]");
}

// --- Test 25: Action Blocks with Statements ---
#[test]
fn test_action_block_statements() {
    grammar! {
        grammar action_block {
            rule main -> i32 = "val" -> {
                let x = 10;
                let y = 20;
                x + y
            }
        }
    }

    action_block::parse_main
        .parse_str("val")
        .test()
        .assert_success_is(30);
}

// --- Test 26: Multi-token Literals (Added in 0.4.0) ---
#[test]
fn test_multi_token_literals() {
    grammar! {
        grammar multi_token {
            // "?." is two tokens: Token![?] and Token![.]
            // The parser generator ensures they are adjacent (no whitespace).
            pub rule optional_dot -> () = "?." -> { () }

            // "@detached" involves a Punct and an Ident.
            // "detached" should be recognized as a custom keyword automatically.
            pub rule at_detached -> () = "@detached" -> { () }

            // "..." is a single token Token![...]
            pub rule dot3 -> () = "..." -> { () }
        }
    }

    // 1. Happy Path: "?."
    multi_token::parse_optional_dot
        .parse_str("?.")
        .test()
        .assert_success();

    // 2. Fail Path: "? ." (Space between tokens)
    // The grammar defined "?." as a single literal, so it enforces adjacency.
    multi_token::parse_optional_dot
        .parse_str("? .")
        .test()
        .assert_failure_contains("found space between tokens");

    // 3. Happy Path: "@detached"
    multi_token::parse_at_detached
        .parse_str("@detached")
        .test()
        .assert_success();

    // 4. Fail Path: "@ detached"
    multi_token::parse_at_detached
        .parse_str("@ detached")
        .test()
        .assert_failure_contains("found space between tokens");

    // 5. Happy Path: "..."
    multi_token::parse_dot3
        .parse_str("...")
        .test()
        .assert_success();
}

// --- Test 27: Use Statements ---
#[test]
fn test_use_statements() {
    grammar! {
        grammar use_statement_test {
            use std::collections::HashMap;

            rule main -> HashMap<String, i32> = "key" -> {
                let mut map = HashMap::new();
                map.insert("key".to_string(), 100);
                map
            }
        }
    }

    use_statement_test::parse_main
        .parse_str("key")
        .test()
        .assert_success();
}

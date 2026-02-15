use syn::parse::Parser;
use syn_grammar::grammar;
use syn_grammar::testing::Testable;

// --- Test Float Primitive ---
#[test]
fn test_float_primitive() {
    grammar! {
        grammar float_test {
            pub rule main -> f64 = f:f64 -> { f }
        }
    }

    // Happy path
    float_test::parse_main
        .parse_str("1.23456")
        .test()
        .assert_success_with(|val| {
            assert!((val - 1.23456).abs() < 1e-6, "Float value mismatch");
        });

    // Integers should fail (syn::LitFloat does not match integer literals unless they have . or exponent)
    float_test::parse_main
        .parse_str("42")
        .test()
        .assert_failure();
}

#[test]
fn test_numeric_primitives() {
    grammar! {
        grammar num_test {
            pub rule test_i8 -> i8 = v:i8 -> { v }
            pub rule test_u64 -> u64 = v:u64 -> { v }
            pub rule test_f32 -> f32 = v:f32 -> { v }
            pub rule test_hex -> u64 = v:hex_literal -> { v }
            pub rule test_oct -> u64 = v:oct_literal -> { v }
            pub rule test_bin -> u64 = v:bin_literal -> { v }
        }
    }

    num_test::parse_test_i8
        .parse_str("127")
        .test()
        .assert_success_is(127i8);
    num_test::parse_test_u64
        .parse_str("1000")
        .test()
        .assert_success_is(1000u64);

    num_test::parse_test_f32
        .parse_str("1.5")
        .test()
        .assert_success_with(|f| {
            assert!((f - 1.5).abs() < 1e-6);
        });

    num_test::parse_test_hex
        .parse_str("0xFF")
        .test()
        .assert_success_is(255u64);
    num_test::parse_test_oct
        .parse_str("0o77")
        .test()
        .assert_success_is(63u64);
    num_test::parse_test_bin
        .parse_str("0b1010")
        .test()
        .assert_success_is(10u64);
}

// --- Test Whitespace Primitive ---
#[test]
fn test_whitespace_primitive() {
    grammar! {
        grammar ws_test {
            // Require whitespace between "a" and "b"
            pub rule main -> () = "a" whitespace "b" -> { () }
        }
    }

    // "a b" -> OK (whitespace exists)
    ws_test::parse_main.parse_str("a b").test().assert_success();

    // "a   b" -> OK
    ws_test::parse_main
        .parse_str("a   b")
        .test()
        .assert_success();
}

#[test]
fn test_whitespace_punct_ident() {
    grammar! {
        grammar ws_punct {
            pub rule main -> () = "@" whitespace "detached" -> { () }
        }
    }

    // "@ detached" -> OK
    ws_punct::parse_main
        .parse_str("@ detached")
        .test()
        .assert_success();

    // "@detached" -> FAIL (adjacent)
    ws_punct::parse_main
        .parse_str("@detached")
        .test()
        .assert_failure_contains("expected whitespace");
}

#[test]
fn test_whitespace_ident_ident() {
    grammar! {
        grammar ws_ident {
            pub rule main -> () = "a" whitespace "b" -> { () }
        }
    }

    ws_ident::parse_main
        .parse_str("a b")
        .test()
        .assert_success();
}

#[test]
fn test_whitespace_between_rules() {
    grammar! {
        grammar ws_rules {
            pub rule main -> () = a whitespace b -> { () }
            rule a -> () = "a" -> { () }
            rule b -> () = "b" -> { () }
        }
    }

    // "a b" -> OK
    ws_rules::parse_main
        .parse_str("a b")
        .test()
        .assert_success();
}

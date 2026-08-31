use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar Primitives {
        pub test_u8 -> u8 = n:u8
        pub test_u16 -> u16 = n:u16
        pub test_u32 -> u32 = n:u32
        pub test_u64 -> u64 = n:u64
        pub test_u128 -> u128 = n:u128
        pub test_usize -> usize = n:usize

        pub test_i8 -> i8 = n:i8
        pub test_i16 -> i16 = n:i16
        pub test_i32 -> i32 = n:i32
        pub test_i64 -> i64 = n:i64
        pub test_i128 -> i128 = n:i128
        pub test_isize -> isize = n:isize

        pub test_f32 -> f32 = n:f32
        pub test_f64 -> f64 = n:f64

        pub test_bool -> bool = b:bool
    }
}

#[test]
fn test_primitives() {
    Primitives::parse_test_u8()
        .parse_test("255")
        .assert_success_is(255);

    Primitives::parse_test_u16()
        .parse_test("65535")
        .assert_success_is(65535);

    Primitives::parse_test_u64()
        .parse_test("18446744073709551615")
        .assert_success_is(u64::MAX);

    Primitives::parse_test_i8()
        .parse_test("-128")
        .assert_success_is(-128);

    Primitives::parse_test_i64()
        .parse_test("-9223372036854775808")
        .assert_success_is(i64::MIN);

    Primitives::parse_test_f32()
        .parse_test("1.5")
        .assert_success_with(|v, _state| assert!((v - 1.5f32).abs() < 1e-6));

    Primitives::parse_test_bool()
        .parse_test("true")
        .assert_success_is(true);

    Primitives::parse_test_bool()
        .parse_test("false")
        .assert_success_is(false);
}

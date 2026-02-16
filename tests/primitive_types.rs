use winnow::{stream::LocatingSlice, Parser};
use winnow_grammar::grammar;

grammar! {
    grammar Primitives {
        pub rule test_u8 -> u8 = n:u8 -> { n }
        pub rule test_u16 -> u16 = n:u16 -> { n }
        pub rule test_u32 -> u32 = n:u32 -> { n }
        pub rule test_u64 -> u64 = n:u64 -> { n }
        pub rule test_u128 -> u128 = n:u128 -> { n }
        pub rule test_usize -> usize = n:usize -> { n }

        pub rule test_i8 -> i8 = n:i8 -> { n }
        pub rule test_i16 -> i16 = n:i16 -> { n }
        pub rule test_i32 -> i32 = n:i32 -> { n }
        pub rule test_i64 -> i64 = n:i64 -> { n }
        pub rule test_i128 -> i128 = n:i128 -> { n }
        pub rule test_isize -> isize = n:isize -> { n }

        pub rule test_f32 -> f32 = n:f32 -> { n }
        pub rule test_f64 -> f64 = n:f64 -> { n }

        pub rule test_bool -> bool = b:bool -> { b }
    }
}

#[test]
fn test_primitives() {
    let input = LocatingSlice::new("255");
    assert_eq!(Primitives::parse_test_u8.parse(input).unwrap(), 255);

    let input = LocatingSlice::new("65535");
    assert_eq!(Primitives::parse_test_u16.parse(input).unwrap(), 65535);

    let input = LocatingSlice::new("18446744073709551615");
    assert_eq!(Primitives::parse_test_u64.parse(input).unwrap(), u64::MAX);

    let input = LocatingSlice::new("-128");
    assert_eq!(Primitives::parse_test_i8.parse(input).unwrap(), -128);

    let input = LocatingSlice::new("-9223372036854775808");
    assert_eq!(Primitives::parse_test_i64.parse(input).unwrap(), i64::MIN);

    let input = LocatingSlice::new("1.5");
    assert!((Primitives::parse_test_f32.parse(input).unwrap() - 1.5f32).abs() < 1e-6);

    let input = LocatingSlice::new("true");
    assert_eq!(Primitives::parse_test_bool.parse(input).unwrap(), true);

    let input = LocatingSlice::new("false");
    assert_eq!(Primitives::parse_test_bool.parse(input).unwrap(), false);
}

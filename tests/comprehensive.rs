use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

#[derive(Debug, PartialEq)]
pub enum Value {
    Int(i32),
    Float(f64),
    String(String),
    Bool(bool),
    List(Vec<Value>),
}

grammar! {
    grammar Comprehensive {
        pub rule value -> Value =
            i:i32 not(".") not("e") not("E") -> { Value::Int(i) }
          | f:f64 -> { Value::Float(f) }
          | s:string -> { Value::String(s.to_string()) }
          | "true" -> { Value::Bool(true) }
          | "false" -> { Value::Bool(false) }
          | "[" l:list_content "]" -> { Value::List(l) }

        rule list_content -> Vec<Value> =
            v:value "," l:list_content -> { let mut l = l; l.insert(0, v); l }
          | v:value -> { vec![v] }
          | empty -> { vec![] }
    }
}

#[test]
fn test_mixed_values() {
    Comprehensive::parse_value()
        .parse_test("123")
        .assert_success_is(Value::Int(123));

    Comprehensive::parse_value()
        .parse_test("123.456")
        .assert_success_with(|v| match v {
            Value::Float(f) => assert!((f - 123.456).abs() < 1e-6),
            _ => panic!("Expected Float for 123.456, got {:?}", v),
        });

    Comprehensive::parse_value()
        .parse_test("123e2")
        .assert_success_with(|v| match v {
            Value::Float(f) => assert!((f - 12300.0).abs() < 1e-6),
            _ => panic!("Expected Float for 123e2, got {:?}", v),
        });

    Comprehensive::parse_value()
        .parse_test("\"hello\"")
        .assert_success_is(Value::String("hello".to_string()));

    Comprehensive::parse_value()
        .parse_test("[1, \"two\", 3.0]")
        .assert_success_with(|v| {
            if let Value::List(l) = v {
                assert_eq!(l.len(), 3);
                assert_eq!(l[0], Value::Int(1));
                assert_eq!(l[1], Value::String("two".to_string()));
                if let Value::Float(f) = l[2] {
                    assert!((f - 3.0).abs() < 1e-6);
                } else {
                    panic!("Expected float at index 2, got {:?}", l[2]);
                }
            } else {
                panic!("Expected list");
            }
        });
}

// Test explicit type usage (generics in return type)
grammar! {
    grammar GenericReturn {
        pub rule optional_int -> Option<i32> =
            i:i32 -> { Some(i) }
          | "none" -> { None }
    }
}

#[test]
fn test_generic_return() {
    GenericReturn::parse_optional_int()
        .parse_test("42")
        .assert_success_is(Some(42));

    GenericReturn::parse_optional_int()
        .parse_test("none")
        .assert_success_is(None);
}

// Test hex/oct/bin parsing manually
grammar! {
    grammar NumFormats {
        pub rule hex -> u32 =
            "0x" h:hex_digit1 -> { u32::from_str_radix(&h, 16).unwrap() }

        pub rule oct -> u32 =
            "0o" o:oct_digit1 -> { u32::from_str_radix(&o, 8).unwrap() }

        pub rule bin -> u32 =
            "0b" b:binary_digit1 -> { u32::from_str_radix(&b, 2).unwrap() }
    }
}

#[test]
fn test_num_formats() {
    NumFormats::parse_hex()
        .parse_test("0x1A")
        .assert_success_is(26);

    NumFormats::parse_oct()
        .parse_test("0o12")
        .assert_success_is(10);

    NumFormats::parse_bin()
        .parse_test("0b1010")
        .assert_success_is(10);
}

// Test i64 parsing
grammar! {
    grammar LargeInt {
        pub rule int64 -> i64 =
            s:digit1 -> { s.parse().unwrap() }
    }
}

#[test]
fn test_int64() {
    LargeInt::parse_int64()
        .parse_test("9223372036854775807")
        .assert_success_is(i64::MAX);
}

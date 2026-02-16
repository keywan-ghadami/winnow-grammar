use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;

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
            i:integer not(".") not("e") not("E") -> { Value::Int(i) }
          | f:float -> { Value::Float(f) }
          | s:string -> { Value::String(s) }
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
    let input = LocatingSlice::new("123");
    let result = Comprehensive::parse_value.parse(input).unwrap();
    assert_eq!(result, Value::Int(123));

    let input = LocatingSlice::new("123.456");
    let result = Comprehensive::parse_value.parse(input).unwrap();
    match result {
        Value::Float(f) => assert!((f - 123.456).abs() < 1e-6),
        _ => panic!("Expected Float for 123.456, got {:?}", result),
    }

    let input = LocatingSlice::new("123e2");
    let result = Comprehensive::parse_value.parse(input).unwrap();
    match result {
        Value::Float(f) => assert!((f - 12300.0).abs() < 1e-6),
        _ => panic!("Expected Float for 123e2, got {:?}", result),
    }

    let input = LocatingSlice::new("\"hello\"");
    let result = Comprehensive::parse_value.parse(input).unwrap();
    assert_eq!(result, Value::String("hello".to_string()));

    let input = LocatingSlice::new("[1, \"two\", 3.0]");
    let result = Comprehensive::parse_value.parse(input).unwrap();
    if let Value::List(l) = result {
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
}

// Test explicit type usage (generics in return type)
grammar! {
    grammar GenericReturn {
        pub rule optional_int -> Option<i32> =
            i:integer -> { Some(i) }
          | "none" -> { None }
    }
}

#[test]
fn test_generic_return() {
    let input = LocatingSlice::new("42");
    assert_eq!(
        GenericReturn::parse_optional_int.parse(input).unwrap(),
        Some(42)
    );

    let input = LocatingSlice::new("none");
    assert_eq!(
        GenericReturn::parse_optional_int.parse(input).unwrap(),
        None
    );
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
    assert_eq!(
        NumFormats::parse_hex
            .parse(LocatingSlice::new("0x1A"))
            .unwrap(),
        26
    );
    assert_eq!(
        NumFormats::parse_oct
            .parse(LocatingSlice::new("0o12"))
            .unwrap(),
        10
    );
    assert_eq!(
        NumFormats::parse_bin
            .parse(LocatingSlice::new("0b1010"))
            .unwrap(),
        10
    );
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
    let input = LocatingSlice::new("9223372036854775807");
    assert_eq!(LargeInt::parse_int64.parse(input).unwrap(), i64::MAX);
}

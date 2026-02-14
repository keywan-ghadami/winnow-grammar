use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;

// -----------------------------------------------------------------------------
// 1. Test Plus (+) Repetition
// -----------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub struct PlusList {
    pub items: Vec<u32>,
}

grammar! {
    grammar TestPlus {
        pub rule list -> PlusList =
            items:uint+ -> { PlusList { items } }
    }
}

#[test]
fn test_plus_repetition() {
    let input = LocatingSlice::new("1 2 3");
    let result = TestPlus::parse_list.parse(input).unwrap();
    assert_eq!(
        result,
        PlusList {
            items: vec![1, 2, 3]
        }
    );

    let input = LocatingSlice::new("1");
    let result = TestPlus::parse_list.parse(input).unwrap();
    assert_eq!(result, PlusList { items: vec![1] });

    let input = LocatingSlice::new("");
    let result = TestPlus::parse_list.parse(input);
    assert!(result.is_err());
}

// -----------------------------------------------------------------------------
// 2. Test Grouping and Alternatives
// -----------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub enum GroupEnum {
    A(u32),
    B(u32),
}

grammar! {
    grammar TestGroup {
        pub rule main -> GroupEnum =
            "a" n:uint -> { GroupEnum::A(n) }
          | "b" n:uint -> { GroupEnum::B(n) }
    }
}

#[test]
fn test_grouping() {
    let input = LocatingSlice::new("a 10");
    let result = TestGroup::parse_main.parse(input).unwrap();
    assert_eq!(result, GroupEnum::A(10));

    let input = LocatingSlice::new("b 20");
    let result = TestGroup::parse_main.parse(input).unwrap();
    assert_eq!(result, GroupEnum::B(20));
}

// -----------------------------------------------------------------------------
// 3. Test Builtins
// -----------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub struct Builtins {
    pub s: String,
    pub i: u32,
    pub id: String,
}

grammar! {
    grammar TestBuiltins {
        pub rule main -> Builtins =
            s:string i:uint id:ident -> { Builtins { s, i, id } }
    }
}

#[test]
fn test_builtins() {
    let input = LocatingSlice::new(r#" "hello" 123 world"#);
    let result = TestBuiltins::parse_main.parse(input).unwrap();
    assert_eq!(
        result,
        Builtins {
            s: "hello".to_string(),
            i: 123,
            id: "world".to_string(),
        }
    );
}

// -----------------------------------------------------------------------------
// 4. Test `use` statements inside grammar
// -----------------------------------------------------------------------------

grammar! {
    grammar TestUse {
        use winnow::token::any;
        use winnow::stream::AsChar;
        pub rule main -> char = c:any -> { c.as_char() }
    }
}

#[test]
fn test_use() {
    let input = LocatingSlice::new("a");
    let result = TestUse::parse_main.parse(input).unwrap();
    assert_eq!(result, 'a');
}

use winnow::prelude::*;
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
        rule list -> PlusList =
            items:integer+ -> { PlusList { items } }
    }
}

#[test]
fn test_plus_repetition() {
    let mut input = "1 2 3";
    let result = TestPlus::parse_list.parse(&mut input).unwrap();
    assert_eq!(result, PlusList { items: vec![1, 2, 3] });

    let mut input = "1";
    let result = TestPlus::parse_list.parse(&mut input).unwrap();
    assert_eq!(result, PlusList { items: vec![1] });

    let mut input = "";
    let result = TestPlus::parse_list.parse(&mut input);
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
        rule main -> GroupEnum =
            "a" ( n:integer -> { GroupEnum::A(n) } )
          | "b" ( n:integer -> { GroupEnum::B(n) } )
    }
}

#[test]
fn test_grouping() {
    let mut input = "a 10";
    let result = TestGroup::parse_main.parse(&mut input).unwrap();
    assert_eq!(result, GroupEnum::A(10));

    let mut input = "b 20";
    let result = TestGroup::parse_main.parse(&mut input).unwrap();
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
        rule main -> Builtins =
            s:string i:integer id:ident -> { Builtins { s, i, id } }
    }
}

#[test]
fn test_builtins() {
    let mut input = r#" "hello" 123 world "#;
    let result = TestBuiltins::parse_main.parse(&mut input).unwrap();
    assert_eq!(result, Builtins {
        s: "hello".to_string(),
        i: 123,
        id: "world".to_string(),
    });
}

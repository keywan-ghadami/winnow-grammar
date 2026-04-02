use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

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
            items:u32+ -> { PlusList { items } }
    }
}

#[test]
fn test_plus_repetition() {
    TestPlus::parse_list()
        .parse_test("1 2 3")
        .assert_success_is(PlusList {
            items: vec![1, 2, 3],
        });

    TestPlus::parse_list()
        .parse_test("1")
        .assert_success_is(PlusList { items: vec![1] });

    TestPlus::parse_list().parse_test("").assert_failure();
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
            "a" n:u32 -> { GroupEnum::A(n) }
          | "b" n:u32 -> { GroupEnum::B(n) }
    }
}

#[test]
fn test_grouping() {
    TestGroup::parse_main()
        .parse_test("a 10")
        .assert_success_is(GroupEnum::A(10));

    TestGroup::parse_main()
        .parse_test("b 20")
        .assert_success_is(GroupEnum::B(20));
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
            s:string i:u32 id:raw_ident -> { Builtins { s: s.to_string(), i, id: id.to_string() } }
    }
}

#[test]
fn test_builtins() {
    TestBuiltins::parse_main()
        .parse_test(r#" "hello" 123 world"#)
        .assert_success_is(Builtins {
            s: "hello".to_string(),
            i: 123,
            id: "world".to_string(),
        });
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
    TestUse::parse_main().parse_test("a").assert_success_is('a');
}

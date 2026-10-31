//! The generated functions' own type parameters (input state, error type,
//! the `each` callback's break value) must not hide a user type of the same
//! name in a rule's type or action. They used to be called `S`, `E` and `B`:
//! `struct S` with `rule top -> S` was rustc's "expected struct, found type
//! parameter `S`".

use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

#[derive(Debug, Clone, PartialEq)]
pub struct S {
    a: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct E(u32);

#[derive(Debug, Clone, PartialEq)]
pub struct B;

grammar! {
    grammar Hygiene {
        pub top -> S = a:i64 -> { S { a } }
        pub err -> E = n:u32 -> { E(n) }
        pub b -> B = "b" -> { B }
        pub all -> (S, E, B) = s:top "," e:err "," b:b -> { (s, e, b) }

        // A generic rule may name its own type parameters `S` and `E`.
        pair<S, E>(l: Rule<S>, r: Rule<E>) -> (S, E) = a:l "," b:r -> { (a, b) }
        pub generic -> (u32, bool) = p:pair<u32, bool>(l=u32, r=flag) -> { p }
        flag -> bool = "t" -> { true } | "f" -> { false }
    }
}

grammar! {
    grammar HygieneStream {
        #[stream]
        pub DOC -> Vec<B> = items:fold(ITEM, Vec::new, |mut v: Vec<B>, i: B| { v.push(i); v }) eof -> { items }
        ITEM -> B = "b" -> { B }
    }
}

#[test]
fn user_types_named_like_type_parameters() {
    Hygiene::parse_top()
        .parse_test("42")
        .assert_success_is(S { a: 42 });
    Hygiene::parse_err().parse_test("7").assert_success_is(E(7));
    Hygiene::parse_b().parse_test("b").assert_success_is(B);
    Hygiene::parse_all()
        .parse_test("1, 2, b")
        .assert_success_is((S { a: 1 }, E(2), B));
    Hygiene::parse_generic()
        .parse_test("3, t")
        .assert_success_is((3, true));
}

//! Rule parameters of reference type: a rule takes a lent value (`&T`),
//! with its lifetime elided or named, and the generated parser captures it.
//! `NoClone` is not `Clone`, so the parameter is passed on as the reference
//! it is, never cloned behind it.

use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

pub struct NoClone {
    offset: usize,
}

pub struct Names {
    known: Vec<&'static str>,
}

grammar! {
    grammar RefParams {
        // An elided lifetime, passed on to a rule with a parameter of its own.
        pub elided(n: &NoClone) -> usize =
            i:usize k:inner(n = n) -> { i + k + n.offset }

        inner(n: &NoClone) -> usize = "+" i:usize -> { i * n.offset }

        // Two elided lifetimes in one rule.
        pub two(n: &NoClone, names: &Names) -> usize =
            w:raw_ident -> { if names.known.contains(&w) { n.offset } else { 0 } }

        // A named lifetime, declared next to a type parameter.
        pub named<'x, T: Clone + Into<usize>>(n: &'x NoClone, t: T) -> usize =
            i:usize -> { i + n.offset + t.clone().into() }
    }
}

#[test]
fn elided_lifetime() {
    let n = NoClone { offset: 3 };
    RefParams::parse_elided(&n)
        .parse_test("1 + 2")
        .assert_success_is(1 + 2 * 3 + 3);
}

#[test]
fn two_elided_lifetimes() {
    let n = NoClone { offset: 7 };
    let names = Names { known: vec!["yes"] };
    RefParams::parse_two(&n, &names)
        .parse_test("yes")
        .assert_success_is(7);
    RefParams::parse_two(&n, &names)
        .parse_test("no")
        .assert_success_is(0);
}

#[test]
fn named_lifetime_with_type_parameter() {
    let n = NoClone { offset: 10 };
    RefParams::parse_named(&n, 5u8)
        .parse_test("1")
        .assert_success_is(16);
}

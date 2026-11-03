//! `name:peek(p)`: what a lookahead matched, bound without consuming it.

use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar Peeked {
        // The value is bound, and the input is still there for what follows.
        pub twice -> (&'a str, &'a str) = a:peek(raw_ident) b:raw_ident -> { (a, b) }

        // A peeked value decides what comes next: a small number is a
        // count, anything else is read as a plain number.
        pub item -> Item<'a> = n:peek(usize) if(n < 10) c:usize "x" w:raw_ident -> { Item::Repeat(c, w) }
                         | n:usize -> { Item::Number(n) }

        // A group passes the value of its single element on.
        pub grouped -> char = c:peek((any)) any -> { c }

        // A lookahead over a sequence still decides alone; only its value is new.
        pub braced -> usize = peek(("{" digit)) "{" n:usize "}" -> { n }
    }
}

#[derive(Debug, PartialEq)]
pub enum Item<'a> {
    Repeat(usize, &'a str),
    Number(usize),
}

#[test]
fn the_value_is_bound_and_the_input_left() {
    Peeked::parse_twice()
        .parse_test("hello")
        .assert_success_is(("hello", "hello"));
}

#[test]
fn a_peeked_value_decides_the_alternative() {
    Peeked::parse_item()
        .parse_test("3 x ab")
        .assert_success_is(Item::Repeat(3, "ab"));
    Peeked::parse_item()
        .parse_test("42")
        .assert_success_is(Item::Number(42));
}

#[test]
fn a_group_passes_its_value_on() {
    Peeked::parse_grouped()
        .parse_test("q")
        .assert_success_is('q');
}

#[test]
fn an_unbound_lookahead_is_unchanged() {
    Peeked::parse_braced()
        .parse_test("{7}")
        .assert_success_is(7);
}

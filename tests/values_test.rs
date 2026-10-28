//! Values in the grammar (ADR 26): arguments that are expressions over the
//! values in scope, repetition bounds that are, and guards.

use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar Args {
        // Lexical rules throughout: a count of spaces is what is tested, and a
        // syntactic rule would skip them.
        // A binding passed on, and an expression over a parameter and a
        // binding.
        pub OUTER -> usize = "a" m:count(" ") v:INNER(m) -> { v }
        INNER(n: usize) -> usize = "x" k:count("y") w:SUM(n, k + 1) -> { w }
        SUM(a: usize, b: usize) -> usize = "" -> { a * 100 + b }

        // Named, and a value shadowing a rule of the same name.
        pub NAMED -> usize = "b" m:count(" ") v:INNER(n = m * 2) -> { v }
        pub SHADOW -> usize = "c" SUM:count(" ") v:INNER(SUM) -> { v }

        // A pattern argument that uses a value in its own argument stays a
        // pattern.
        pub NESTED -> usize = "d" m:count(" ") v:WRAP(p = INNER(m)) -> { v }
        WRAP<T>(p: Rule<T>) -> T = v:p -> { v }

        // A value that is not `Copy`, passed on in a repetition.
        pub NAMES -> Vec<String> = p:text("p") ":" v:NAME(prefix = p.to_string())* -> { v }
        NAME(prefix: String) -> String = " " n:text(("x" | "y")) -> { format!("{prefix}{n}") }
    }
}

#[test]
fn a_binding_reaches_the_rule_it_is_passed_to() {
    Args::parse_OUTER()
        .parse_test("a   xyy")
        .assert_success_is(303);
    Args::parse_OUTER().parse_test("ax").assert_success_is(1);
    Args::parse_NAMED()
        .parse_test("b  x")
        .assert_success_is(401);
    Args::parse_SHADOW()
        .parse_test("c x")
        .assert_success_is(101);
    Args::parse_NESTED()
        .parse_test("d  xy")
        .assert_success_is(202);
}

grammar! {
    grammar Bounds {
        // Exactly `n`, at most `n - 1` (`s-indent(<n)`), at least `n`.
        pub EXACT -> usize = n:dec<usize>(digit) ":" s:INDENT(n) "x" -> { s }
        INDENT(n: usize) -> usize = s:" "{n} -> { s.len() }
        pub FEWER -> &'a str = n:dec<i32>(digit) ":" s:text(" "{0, n - 1}) "x" -> { s }
        pub ATLEAST -> &'a str = n:dec<u8>(digit) ":" s:text(" "{n,}) "x" -> { s }
        // A run of a character class with a computed bound is still text.
        pub DIGITS -> &'a str = n:dec<usize>(digit) ":" d:digit{n} -> { d }
        // Two alternatives that differ only in a computed bound are two, not
        // one written twice.
        pub PICK -> u8 = n:dec<usize>(digit) ":" v:PICKED(n) -> { v }
        PICKED(n: usize) -> u8 = "a"{n + 1} -> { 2 } | "a"{n} -> { 1 }
    }
}

#[test]
fn a_computed_bound_counts_what_it_says() {
    Bounds::parse_EXACT()
        .parse_test("3:   x")
        .assert_success_is(3);
    Bounds::parse_EXACT().parse_test("0:x").assert_success_is(0);
    Bounds::parse_EXACT().parse_test("3:  x").assert_failure();
    // Greedy and possessive, as a literal bound: four spaces leave one.
    Bounds::parse_EXACT().parse_test("3:    x").assert_failure();

    Bounds::parse_FEWER()
        .parse_test("3:  x")
        .assert_success_is("  ");
    Bounds::parse_FEWER()
        .parse_test("1:x")
        .assert_success_is("");
    Bounds::parse_FEWER().parse_test("1: x").assert_failure();
    // `n - 1` is below zero: no count is possible, not a wrapped one.
    Bounds::parse_FEWER().parse_test("0:x").assert_failure();

    Bounds::parse_ATLEAST()
        .parse_test("2:     x")
        .assert_success_is("     ");
    Bounds::parse_ATLEAST().parse_test("2: x").assert_failure();

    Bounds::parse_DIGITS()
        .parse_test("3:123")
        .assert_success_is("123");
    Bounds::parse_DIGITS().parse_test("3:12").assert_failure();

    Bounds::parse_PICK().parse_test("1:aa").assert_success_is(2);
    Bounds::parse_PICK().parse_test("1:a").assert_success_is(1);
}

grammar! {
    grammar Records {
        input bytes;
        // A length in front of its payload.
        pub RECORD -> &'a [u8] = n:u8 ":" data:bytes(byte{n}) -> { data }
    }
}

#[test]
fn a_length_in_front_of_its_payload() {
    let run = |input: &'static [u8]| {
        let mut i = winnow_grammar::ParseBytes {
            input: winnow::stream::LocatingSlice::new(input),
            state: winnow_grammar::ParseContext::<()>::default(),
        };
        winnow::Parser::parse_next(&mut Records::parse_RECORD(), &mut i).ok()
    };
    assert_eq!(run(b"3:a\xffb"), Some(&b"a\xffb"[..]));
    assert_eq!(run(b"0:"), Some(&b""[..]));
    assert_eq!(run(b"3:ab"), None);
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ctx {
    Block,
    Flow,
}

grammar! {
    grammar Guards {
        // More indented than the parent, or the next alternative.
        pub DEEPER -> &'a str =
            n:dec<usize>(digit) ":" m:count(" ") if(m > n) "x" -> { "deeper" }
          | dec<usize>(digit) ":" " "* "x" -> { "not" }

        // A context as a value, and a guard with a message.
        pub BLOCK -> &'a str = v:ITEM(c = super::Ctx::Block) -> { v }
        pub FLOW -> &'a str = v:ITEM(c = super::Ctx::Flow) -> { v }
        ITEM(c: super::Ctx) -> &'a str =
            if(c == super::Ctx::Block, "a block context") "-" -> { "block" }
          | "[" -> { "flow" }

        // After a cut, a guard that fails is a cut like anything else.
        pub COMMITTED -> u8 = "k" => n:dec<u8>(digit) if(n < 5) -> { n } | "k9" -> { 99 }
    }
}

#[test]
fn a_guard_lets_the_next_alternative_be_tried() {
    Guards::parse_DEEPER()
        .parse_test("1:  x")
        .assert_success_is("deeper");
    Guards::parse_DEEPER()
        .parse_test("2:  x")
        .assert_success_is("not");
    Guards::parse_DEEPER()
        .parse_test("0:x")
        .assert_success_is("not");

    Guards::parse_BLOCK()
        .parse_test("-")
        .assert_success_is("block");
    Guards::parse_FLOW()
        .parse_test("[")
        .assert_success_is("flow");
    Guards::parse_FLOW()
        .parse_test("-")
        .assert_failure_contains("a block context");

    Guards::parse_COMMITTED()
        .parse_test("k3")
        .assert_success_is(3);
    Guards::parse_COMMITTED()
        .parse_test("k9")
        .assert_failure_contains("`n < 5`");
}

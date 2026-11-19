//! A discarded group whose alternatives yield different values - `x:any` a
//! `char`, `":"` a `&str` - still compiles: nobody looks at the value, so each
//! alternative is voided before `alt` joins them.

use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

fn ok(c: char, flow: bool) -> bool {
    c != ':' && !(flow && c == ',')
}

grammar! {
    grammar Alts {
        // Under `text(…)`: a run of `any` or `":"`.
        pub TEXT -> &'a str = s:text((any | ":")+) -> { s }

        // A guarded character or a `:` followed by a safe one, repeated, in a
        // rule nobody takes a value from.
        pub RUN -> &'a str = s:text(Run(flow = false)) -> { s }
        Run(flow: bool) -> () = (x:any if(ok(x, flow)) | ":" peek(Safe(flow = flow)))+ -> { }
        Safe(flow: bool) -> () = x:any if(ok(x, flow)) -> { }

        // The same unlike alternatives directly in a sequence, nobody taking
        // the group's value.
        pub SEQ -> &'a str = s:text(Seq) -> { s }
        Seq -> () = "<" (x:any if(ok(x, false)) | ":") ">" -> { }

        // ... and under bounds.
        pub BOUNDED -> &'a str = s:text(Bounded) -> { s }
        Bounded -> () = (x:any if(ok(x, false)) | ":"){1,3} ";" -> { }

        // A bound repetition of like alternatives still collects them.
        pub BOUND -> Vec<&'a str> = v:("x" | "y")+ -> { v }
    }
}

#[test]
fn a_discarded_group_of_unlike_alternatives_parses() {
    Alts::parse_TEXT()
        .parse_test("ab:c")
        .assert_success_is("ab:c");
    Alts::parse_RUN()
        .parse_test("ab:c")
        .assert_success_is("ab:c");
}

#[test]
fn a_discarded_group_in_a_sequence_parses() {
    Alts::parse_SEQ().parse_test("<a>").assert_success_is("<a>");
    Alts::parse_SEQ().parse_test("<:>").assert_success_is("<:>");
    Alts::parse_SEQ().parse_test("<ab>").assert_failure();
}

#[test]
fn a_discarded_bounded_group_keeps_its_bounds() {
    Alts::parse_BOUNDED()
        .parse_test("a:b;")
        .assert_success_is("a:b;");
    Alts::parse_BOUNDED().parse_test(";").assert_failure();
    Alts::parse_BOUNDED().parse_test("a:bc;").assert_failure();
}

#[test]
fn a_bound_group_still_collects() {
    Alts::parse_BOUND()
        .parse_test("xyx")
        .assert_success_is(vec!["x", "y", "x"]);
}

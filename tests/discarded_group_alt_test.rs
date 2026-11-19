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

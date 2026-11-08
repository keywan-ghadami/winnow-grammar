//! The trivia a syntactic rule skips after a cut `=>` is under the cut.
//!
//! A `WS` that can fail - here: never in front of a `~` - failing between two
//! elements after the cut is a cut failure, so no alternative is tried after
//! it. Before, the skip was a plain backtrack, and `"k" => "a" "b"` on
//! `ka~b` handed over to the next alternative although `k` had committed.
//! The skip in front of the `=>` itself comes before the commitment and
//! still backtracks. Each case is written once as a rule's own sequence
//! (statements) and once inside an option (combinators).

use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar TriviaCut {
        WS -> () = " "* not("~") -> { () }

        // A rule's own sequence.
        pub stmt -> u32 =
            "k" => "a" "b" -> { 1 }
          | "k" "a~b" -> { 2 }

        // The same sequence inside an option.
        pub opt -> u32 = _o:("k" => "a" "b")? "ka~b" -> { 2 }

        // The skip in front of the cut is not under it.
        pub before -> u32 =
            "k" "a" => "b" -> { 1 }
          | "ka~" -> { 2 }

        // `paren(…)` with a cut inside: the skip before the inner pattern's
        // element and the closing `)` are under it - as a rule's step and
        // inside an option.
        pub delim -> u32 =
            paren("k" => "a" "b") -> { 1 }
          | "(ka~b)" -> { 2 }
        pub delim_opt -> u32 = _o:paren("k" => "a")? "(ka]" -> { 2 }
    }
}

#[test]
fn a_failing_skip_after_a_cut_is_a_cut_failure() {
    TriviaCut::parse_stmt()
        .parse_test("k a b")
        .assert_success_is(1);
    TriviaCut::parse_stmt().parse_test("ka~b").assert_failure();
}

#[test]
fn a_failing_skip_after_a_cut_is_a_cut_failure_inside_an_option() {
    TriviaCut::parse_opt()
        .parse_test("k a b ka~b")
        .assert_success_is(2);
    TriviaCut::parse_opt().parse_test("ka~b").assert_failure();
}

#[test]
fn the_skip_in_front_of_the_cut_still_backtracks() {
    TriviaCut::parse_before()
        .parse_test("k a b")
        .assert_success_is(1);
    TriviaCut::parse_before()
        .parse_test("ka~")
        .assert_success_is(2);
}

#[test]
fn a_cut_in_a_delimiter_holds_to_its_close() {
    TriviaCut::parse_delim()
        .parse_test("( k a b )")
        .assert_success_is(1);
    TriviaCut::parse_delim()
        .parse_test("(ka~b)")
        .assert_failure();
    TriviaCut::parse_delim_opt()
        .parse_test("(k a) (ka]")
        .assert_success_is(2);
    // `)` fails after the cut inside: a cut failure, not a way out.
    TriviaCut::parse_delim_opt()
        .parse_test("(ka]")
        .assert_failure();
}

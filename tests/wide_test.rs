//! Wider than a tuple: winnow 1.0 implements `alt` for at most nine parsers and
//! `Parser` for tuples of at most eleven. The generator nests what is wider,
//! so a grammar does not see the limit.

use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar Wide {
        // Twenty-five alternatives of a rule.
        pub word -> u32 =
            "a" -> { 1 } | "b" -> { 2 } | "c" -> { 3 } | "d" -> { 4 } | "e" -> { 5 }
          | "f" -> { 6 } | "g" -> { 7 } | "h" -> { 8 } | "i" -> { 9 } | "j" -> { 10 }
          | "k" -> { 11 } | "l" -> { 12 } | "m" -> { 13 } | "n" -> { 14 } | "o" -> { 15 }
          | "p" -> { 16 } | "q" -> { 17 } | "r" -> { 18 } | "s" -> { 19 } | "t" -> { 20 }
          | "u" -> { 21 } | "v" -> { 22 } | "w" -> { 23 } | "x" -> { 24 } | "y" -> { 25 }

        // Twenty-five alternatives of a group.
        pub grouped -> &'a str =
            s:("a" | "b" | "c" | "d" | "e" | "f" | "g" | "h" | "i" | "j" | "k" | "l" | "m"
              | "n" | "o" | "p" | "q" | "r" | "s" | "t" | "u" | "v" | "w" | "x" | "y") ";"
            -> { s }

        // A sequence of fifteen elements, with the whitespace skips between
        // them nearly thirty parsers.
        pub long -> u32 =
            a:u32 "," b:u32 "," c:u32 "," d:u32 "," e:u32 "," f:u32 "," g:u32 "," h:u32
            -> { a + b + c + d + e + f + g + h }

        // The same in a lexical rule, where nothing is skipped.
        pub LEX -> &'a str =
            "a" "b" "c" "d" "e" "f" "g" "h" "i" "j" "k" "l" "m" "n" s:"o" -> { s }

        // A group of a long sequence in a rule that skips whitespace: the
        // skips between its elements make it more than twenty parsers.
        pub seq_group -> u32 =
            ("a" "b" "c" "d" "e" "f" "g" "h" "i" "j" "k" "l") n:u32 -> { n }

        // A long sequence with a cut in it.
        pub cut -> u32 =
            "k" "k" "k" "k" "k" "k" => a:u32 "," b:u32 "," c:u32 "," d:u32 -> { a + b + c + d }
          | "k" -> { 0 }
    }
}

#[test]
fn every_alternative_of_a_wide_rule_is_reached() {
    for (i, c) in ("abcdefghijklmnopqrstuvwxy").char_indices() {
        let s = c.to_string();
        Wide::parse_word()
            .parse_test(&s)
            .assert_success_is(i as u32 + 1);
    }
    Wide::parse_word().parse_test("z").assert_failure();
}

#[test]
fn every_alternative_of_a_wide_group_is_reached() {
    Wide::parse_grouped()
        .parse_test("a;")
        .assert_success_is("a");
    Wide::parse_grouped()
        .parse_test("k;")
        .assert_success_is("k");
    Wide::parse_grouped()
        .parse_test("y;")
        .assert_success_is("y");
    Wide::parse_grouped().parse_test("z;").assert_failure();
}

#[test]
fn a_long_sequence_parses_in_order() {
    Wide::parse_long()
        .parse_test("1, 2, 3, 4, 5, 6, 7, 8")
        .assert_success_is(36);
    Wide::parse_long().parse_test("1, 2, 3").assert_failure();
    Wide::parse_LEX()
        .parse_test("abcdefghijklmno")
        .assert_success_is("o");
}

#[test]
fn a_long_group_parses_in_order() {
    Wide::parse_seq_group()
        .parse_test("a b c d e f g h i j k l 7")
        .assert_success_is(7);
    Wide::parse_seq_group()
        .parse_test("a b c d e f g h i j l k 7")
        .assert_failure();
}

#[test]
fn a_cut_in_a_long_sequence_still_commits() {
    Wide::parse_cut()
        .parse_test("kkkkkk 1,2,3,4")
        .assert_success_is(10);
    Wide::parse_cut().parse_test("k").assert_success_is(0);
    // Past the cut a bad tail is fatal, not a reason to try `"k"`.
    Wide::parse_cut().parse_test("kkkkkk 1,2").assert_failure();
}

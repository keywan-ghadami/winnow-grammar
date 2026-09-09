//! What the cut operator `=>` commits to.
//!
//! The item asked for verification rather than a rewrite: the generator sets a
//! flag at the cut and wraps every later step of the sequence in `cut_err`,
//! and the question was whether that is right at the edges - inside groups,
//! delimiters, repetitions, and between the alternatives of a rule. These
//! tests answer it by behaviour.

use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar Cut {
        // The plain case: once `let` is seen, this alternative is the answer,
        // so a bad tail is an error rather than a reason to try the next one.
        pub stmt -> &'a str =
            "let" => n:raw_ident ";" -> { n }
          | "let" "!" -> { "bang" }
          | n:raw_ident -> { n }

        // Without a cut the same shape backtracks into the second alternative.
        pub soft -> &'a str =
            "let" n:raw_ident ";" -> { n }
          | "let" "!" -> { "bang" }

        // A cut inside a rule reached from a sequence: it commits within that
        // rule, and the caller is not committed by it.
        pub grouped -> &'a str = "(" g:inner ")" -> { g }
        rule inner -> &'a str =
            "!" => n:raw_ident -> { n }
          | n:raw_ident -> { n }

        // A cut inside the element of a repetition: each element commits on
        // its own, and the repetition still ends where the element no longer
        // starts.
        pub items -> usize = xs:elem* "c" -> { xs.len() }
        rule elem -> usize = "a" => "b" -> { 1 }

        // A cut before a call to a rule with alternatives: the call as a whole
        // is committed to, and the alternatives still choose between
        // themselves.
        pub after -> u32 = "k" => v:num "!" -> { v }
        rule num -> u32 =
            v:u32 -> { v }
          | "-" v:u32 -> { v }
    }
}

#[test]
fn a_cut_commits_to_its_alternative() {
    // `let x;` is the first alternative.
    Cut::parse_stmt()
        .parse_test("let x;")
        .assert_success_is("x");
    // A bare identifier is the third.
    Cut::parse_stmt()
        .parse_test("plain")
        .assert_success_is("plain");

    // `let !` matches the *second* alternative's shape - but the cut in the
    // first has already committed, so this is an error rather than a match.
    // The error is the committed alternative's, not an `expected one of`
    // gathered across all three.
    Cut::parse_stmt()
        .parse_test("let !")
        .assert_failure_contains("identifier");
}

#[test]
fn without_a_cut_the_same_shape_backtracks() {
    // The control. If this ever fails the same way as the test above, the cut
    // is not what makes the difference.
    Cut::parse_soft()
        .parse_test("let !")
        .assert_success_is("bang");
}

#[test]
fn a_cut_inside_a_group_commits_inside_it() {
    Cut::parse_grouped()
        .parse_test("(!x)")
        .assert_success_is("x");
    Cut::parse_grouped()
        .parse_test("(x)")
        .assert_success_is("x");
    // `(!)` - the cut has committed to needing an identifier after `!`, so the
    // second alternative of the group is not tried.
    Cut::parse_grouped().parse_test("(!)").assert_failure();
}

#[test]
fn a_cut_inside_a_repetition_ends_the_element_not_the_loop() {
    Cut::parse_items().parse_test("ababc").assert_success_is(2);
    Cut::parse_items().parse_test("c").assert_success_is(0);
    // `a` without its `b` is fatal *inside the element*, and a fatal element
    // is not "the repetition ended" - the parse fails rather than stopping
    // with one item.
    Cut::parse_items().parse_test("abac").assert_failure();
}

#[test]
fn a_cut_before_a_group_leaves_the_group_free_to_choose() {
    Cut::parse_after().parse_test("k12!").assert_success_is(12);
    Cut::parse_after().parse_test("k-12!").assert_success_is(12);
    // Neither alternative matches: fatal, because of the cut before the group.
    Cut::parse_after().parse_test("kx!").assert_failure();
}

// -----------------------------------------------------------------------------
// How far a cut reaches. §1's goal says it should "commit to the current
// alternative without bleeding into unrelated parsing paths" - so the question
// is what happens to a *caller* whose alternative contains a rule that cut.
// -----------------------------------------------------------------------------

grammar! {
    grammar Reach {
        // `keyed` cuts after `k`.
        rule keyed -> u32 = "k" => v:u32 -> { v }

        // A caller with an alternative that does not involve `keyed` at all.
        pub caller -> u32 =
            v:keyed "!" -> { v }
          | "k" "?" -> { 999 }
    }
}

#[test]
fn a_cut_reaches_past_the_rule_it_is_written_in() {
    Reach::parse_caller()
        .parse_test("k12!")
        .assert_success_is(12);

    // `k?` matches the second alternative's shape. But `keyed` is tried first,
    // its cut fires after `k`, and the resulting error is fatal - so the
    // second alternative is never tried.
    Reach::parse_caller().parse_test("k?").assert_failure();
}

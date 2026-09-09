//! `# "…"` on a rule: what it is called when it fails where it began.
//!
//! The same syntax after an alternative labels only that one, which is no help
//! where it is needed most - a rule with a dozen alternatives, whose failure
//! reports a dozen token spellings when one word would do. A label on the rule
//! covers all of them, and only at the rule's own starting position: a rule
//! that got further was in the middle of something, and what it was in the
//! middle of is the more informative message.

use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar Lang {
        pub rule doc -> i64 = e:expr -> { e }

        rule expr -> i64 # "expression" =
              n:i64 -> { n }
            | "(" e:expr ")" -> { e }
            | "-" e:expr -> { -e }
    }
}

#[test]
fn a_labelled_rule_names_itself_instead_of_its_alternatives() {
    let e = Lang::parse_doc().parse_test("x").inner.unwrap_err();
    assert!(e.starts_with("expected expression"), "{e}");
    // The alternatives' own spellings are gone, not merely outranked: the
    // label *is* the expectation.
    assert!(!e.contains("`(`"), "{e}");
    assert!(!e.contains("integer"), "{e}");
}

#[test]
fn the_label_survives_the_whitespace_a_syntactic_rule_skips() {
    // The trap this was written against: a syntactic rule skips whitespace at
    // its start, so a failure sits *past* the blanks. Measured from inside the
    // label the offsets would not match and the label would never substitute.
    let e = Lang::parse_doc().parse_test("   ").inner.unwrap_err();
    assert!(e.contains("expected expression"), "{e}");
}

#[test]
fn a_rule_that_got_further_keeps_its_own_message() {
    // The `(` matched, so the failure is inside an alternative rather than at
    // the rule's start - and `expected expression` would be a worse message
    // than the one that names the missing `)`.
    let e = Lang::parse_doc().parse_test("(1").inner.unwrap_err();
    assert!(e.contains("`)`"), "{e}");
}

#[test]
fn a_labelled_rule_still_parses_what_it_always_did() {
    Lang::parse_doc().parse_test("-(1)").assert_success_is(-1);
    Lang::parse_doc().parse_test("  42 ").assert_success_is(42);
}

//! `_state` in an action block: the escape hatch of ADR 18 §2.
//!
//! An action whose text mentions `_state` gets a `&mut ParseContext` bound
//! ahead of it. The injection used to emit `Stateful::state_mut`, which winnow
//! 0.7 does not have, so every grammar that named `_state` failed to compile -
//! and nothing named it. These tests name it, once per injection site.

use winnow_grammar::testing::WinnowTestExt;
use winnow_grammar::{grammar, Symbol};

grammar! {
    grammar StateAction {
        // 1. A plain variant: intern something `ident` cannot reach - a
        //    quoted field value, kept verbatim.
        pub field -> Symbol =
            "\"" s:until("\"") "\"" -> { _state.interner.intern_string(s) }

        // Two of them, to show the symbols come from one interner.
        pub pair -> (Symbol, Symbol) = a:field b:field -> { (a, b) }

        // 2. A variant with a span binding (`@`), which takes the other
        //    injection site in `generate_variants_body`.
        pub spanned -> (Symbol, usize) =
            s:until("!") @ sp "!" -> { (_state.interner.intern_string(s), sp.start) }

        // 3. A left-recursive rule: the injection site in the loop body.
        pub chain -> Symbol =
            l:chain "+" r:word -> {
                let joined = format!("{}{}", _state.interner.resolve(l), _state.interner.resolve(r));
                _state.interner.intern_string(&joined)
            }
          | w:word -> { w }

        rule word -> Symbol = s:alpha1 -> { _state.interner.intern_string(s) }
    }
}

#[test]
fn an_action_interns_through_state() {
    StateAction::parse_field()
        .parse_test("\"Hamburg\"")
        .assert_success_with(|s, ctx| assert_eq!(ctx.interner.resolve(*s), "Hamburg"));
}

#[test]
fn two_actions_share_the_contexts_interner() {
    StateAction::parse_pair()
        .parse_test("\"Hamburg\" \"Hamburg\"")
        .assert_success_with(|(a, b), ctx| {
            assert_eq!(a, b);
            assert_eq!(ctx.interner.resolve(*a), "Hamburg");
        });
}

#[test]
fn state_and_span_in_the_same_action() {
    StateAction::parse_spanned()
        .parse_test("Hamburg!")
        .assert_success_with(|(s, start), ctx| {
            assert_eq!(ctx.interner.resolve(*s), "Hamburg");
            assert_eq!(*start, 0);
        });
}

#[test]
fn state_in_a_left_recursive_action() {
    StateAction::parse_chain()
        .parse_test("a + b + c")
        .assert_success_with(|s, ctx| assert_eq!(ctx.interner.resolve(*s), "abc"));
}

// The binding is injected unconditionally, so the word `_state` in a comment
// or a string literal is just text - it used to trigger the injection, and an
// action that wanted the name for itself could not have it.
grammar! {
    grammar StateIsNotAKeyword {
        pub mentions -> &'a str = w:alpha1 -> {
            // _state in a comment
            let _ = "_state in a string";
            w
        }

        // An action may bind the name itself; the injected one is shadowed.
        pub shadows -> usize = w:alpha1 -> {
            let _state = w.len();
            _state * 2
        }
    }
}

#[test]
fn the_word_state_in_a_comment_or_a_string_is_just_text() {
    StateIsNotAKeyword::parse_mentions()
        .parse_test("abc")
        .assert_success_is("abc");
}

#[test]
fn an_action_may_bind_the_name_itself() {
    StateIsNotAKeyword::parse_shadows()
        .parse_test("abcd")
        .assert_success_is(8);
}

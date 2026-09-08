//! The `intern(p)` builtin - ADR 18 §1.

use winnow_grammar::testing::WinnowTestExt;
use winnow_grammar::{grammar, Symbol};

grammar! {
    grammar Interning {
        // The 1BRC shape: a field value is not an identifier, and before
        // `intern` there was no way to say this in the grammar.
        pub city -> Symbol = s:intern(until(";")) -> { s }

        pub row -> (Symbol, i32) = c:city ";" t:i32 -> { (c, t) }

        // Whatever yields text can be interned.
        pub quoted -> Symbol = s:intern(string) -> { s }
        pub word -> Symbol = s:intern(alpha1) -> { s }
        pub name -> Symbol = s:intern(raw_ident) -> { s }

        // A user rule as the argument.
        rule two_letters -> &'a str = s:alpha1 -> { s }
        pub from_rule -> Symbol = s:intern(two_letters) -> { s }

        // Interned inside a repetition: the same word twice is one symbol.
        pub words -> Vec<Symbol> = ws:word* -> { ws }
    }
}

#[test]
fn interns_a_field_value_that_is_not_an_identifier() {
    Interning::parse_row()
        .parse_test("São Paulo;-12")
        .assert_success_with(|(c, t), ctx| {
            assert_eq!(ctx.interner.resolve(*c), "São Paulo");
            assert_eq!(*t, -12);
        });
}

#[test]
fn interns_whatever_yields_text() {
    Interning::parse_quoted()
        .parse_test("\"hello\"")
        .assert_success_with(|s, ctx| assert_eq!(ctx.interner.resolve(*s), "hello"));
    Interning::parse_word()
        .parse_test("hello")
        .assert_success_with(|s, ctx| assert_eq!(ctx.interner.resolve(*s), "hello"));
    Interning::parse_name()
        .parse_test("hello_1")
        .assert_success_with(|s, ctx| assert_eq!(ctx.interner.resolve(*s), "hello_1"));
    Interning::parse_from_rule()
        .parse_test("abc")
        .assert_success_with(|s, ctx| assert_eq!(ctx.interner.resolve(*s), "abc"));
}

#[test]
fn the_same_text_is_the_same_symbol_and_different_text_is_not() {
    Interning::parse_words()
        .parse_test("alpha beta alpha")
        .assert_success_with(|ws, ctx| {
            assert_eq!(ws.len(), 3);
            assert_eq!(ws[0], ws[2]);
            assert_ne!(ws[0], ws[1]);
            assert_eq!(ctx.interner.resolve(ws[1]), "beta");
        });
}

#[test]
fn a_failing_inner_pattern_fails_the_intern() {
    Interning::parse_word().parse_test("123").assert_failure();
}

// `ident` is `intern(raw_ident)` - the two must agree, including on the
// symbol, since both go through the context's one interner.
grammar! {
    grammar IdentIsInternRawIdent {
        pub both -> (Symbol, Symbol) = a:ident "," b:intern(raw_ident) -> { (a, b) }
    }
}

#[test]
fn ident_is_intern_raw_ident() {
    IdentIsInternRawIdent::parse_both()
        .parse_test("hello, hello")
        .assert_success_with(|(a, b), ctx| {
            assert_eq!(a, b);
            assert_eq!(ctx.interner.resolve(*a), "hello");
        });
}

// `intern` inside a `#[frame]` rule: the frame check looks through it to the
// argument, which is where the boundary is or is not covered. An opaque
// `intern` would reject this grammar.
grammar! {
    grammar Framed {
        WS -> () = "" -> { () }

        #[frame(boundary = "\n")]
        pub ROW -> Symbol = c:intern(until(";" | frame_end)) ";" digit1 frame_end -> { c }

        pub FILE -> Vec<Symbol> = s:par_fold(
            ROW,
            Vec::new,
            |mut acc: Vec<Symbol>, c: Symbol| { acc.push(c); acc },
            |mut a: Vec<Symbol>, b: Vec<Symbol>| { a.extend(b); a }
        ) -> { s }
    }
}

#[test]
fn intern_is_transparent_to_the_frame_check() {
    Framed::parse_FILE()
        .parse_test("Hamburg;1\nZurich;2\n")
        .assert_success_with(|syms, ctx| {
            assert_eq!(ctx.interner.resolve(syms[0]), "Hamburg");
            assert_eq!(ctx.interner.resolve(syms[1]), "Zurich");
        });
}

// -----------------------------------------------------------------------------
// Interning is monotone: a branch that interns and then loses leaves its entry
// behind. Asserted in ADR 18's consequences; here it is measured, because it is
// also what ADR 20 has to say about writing to a state from an action.
// -----------------------------------------------------------------------------

grammar! {
    grammar Backtracks {
        // The first alternative interns and *then* fails; the second wins.
        pub value -> Symbol =
            s:intern(alpha1) "!" -> { s }
          | s:intern(alpha1) "?" -> { s }
    }
}

#[test]
fn a_backtracked_alternative_has_still_interned() {
    Backtracks::parse_value()
        .parse_test("hello?")
        .assert_success_with(|s, ctx| {
            assert_eq!(ctx.interner.resolve(*s), "hello");
            // Both alternatives interned the same text, so this says nothing
            // yet - the point is the count.
            assert_eq!(ctx.interner.len(), 1);
        });
}

grammar! {
    grammar Distinct {
        // The losing branch interns something the winning one never sees.
        pub value -> Symbol =
            _a:intern(alpha1) "-" b:intern(alpha1) "!" -> { b }
          | a:intern(alpha1) "-" _b:intern(digit1) "?" -> { a }
    }
}

#[test]
fn what_a_lost_branch_interned_stays_in_the_interner() {
    Distinct::parse_value()
        .parse_test("alpha-12?")
        .assert_success_with(|s, ctx| {
            assert_eq!(ctx.interner.resolve(*s), "alpha");
            // "alpha" from both branches, plus "12" from the winner. If the
            // losing branch had interned a word of its own it would be here
            // too: interning is monotone, a backtrack does not undo it.
            assert_eq!(ctx.interner.len(), 2, "alpha, 12");
        });
}

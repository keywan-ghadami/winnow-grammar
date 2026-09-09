//! `interner I;` - the interner `ident` and `intern(…)` use (ADR 22).
//!
//! Before this, a grammar could put an interner of its own in the state and
//! reach it from a hand-written parser, but the language's own interning
//! operators still meant `ParseContext::interner`. Declaring one changes what
//! they mean, and nothing else: `Symbol` stays the identity, so every rule's
//! return type is unchanged.

use winnow_grammar::testing::{WinnowTestExt, WinnowTestExtWith};
use winnow_grammar::{grammar, Interner, InternerOf, Symbol};

/// A direct-index table: the slot number *is* the identity, which is what a
/// 1BRC-class solution wants and what `benches/where.rs` measures at ~7 ns
/// against the general interner's ~21.
#[derive(Clone, Debug, Default)]
struct Slots {
    names: Vec<String>,
}

impl Interner for Slots {
    fn intern(&mut self, text: &str) -> Symbol {
        if let Some(i) = self.names.iter().position(|n| n == text) {
            return Symbol::from_index(i as u32);
        }
        self.names.push(text.to_string());
        Symbol::from_index(self.names.len() as u32 - 1)
    }

    fn resolve(&self, symbol: Symbol) -> &str {
        &self.names[symbol.index() as usize]
    }
}

grammar! {
    grammar Named {
        interner Slots;

        pub two -> (Symbol, Symbol) = a:ident b:ident -> { (a, b) }

        pub field -> Symbol = s:intern(alpha1) -> { s }
    }
}

#[test]
fn ident_interns_into_the_declared_interner() {
    Named::parse_two()
        .parse_test_in(Slots::default(), "alpha beta")
        .assert_success_with(|(a, b), ctx| {
            // The numbers came from `Slots`, in the order it saw them.
            assert_eq!(a.index(), 0);
            assert_eq!(b.index(), 1);
            assert_eq!(ctx.user_state.names, ["alpha", "beta"]);
            // And the context's own interner was never touched.
            assert!(ctx.interner.is_empty());
        });
}

#[test]
fn the_same_word_is_the_same_symbol() {
    Named::parse_two()
        .parse_test_in(Slots::default(), "same same")
        .assert_success_with(|(a, b), ctx| {
            assert_eq!(a, b);
            assert_eq!(ctx.user_state.names.len(), 1);
            assert_eq!(ctx.user_state.resolve(*a), "same");
        });
}

#[test]
fn intern_of_a_pattern_goes_the_same_way() {
    Named::parse_field()
        .parse_test_in(Slots::default(), "hello")
        .assert_success_with(|s, ctx| {
            assert_eq!(s.index(), 0);
            assert_eq!(ctx.user_state.names, ["hello"]);
        });
}

// One state serving a grammar's `state` and another's `interner`: the reason
// both are bounds rather than type parameters.
#[derive(Clone, Debug, Default)]
struct Counted {
    n: usize,
}

#[derive(Clone, Debug, Default)]
struct App {
    slots: Slots,
    counted: Counted,
}

impl InternerOf<Slots> for App {
    fn interner(&mut self) -> &mut Slots {
        &mut self.slots
    }
}

impl winnow_grammar::StateOf<Counted> for App {
    fn state(&mut self) -> &mut Counted {
        &mut self.counted
    }
}

grammar! {
    grammar Both {
        interner Slots;
        state Counted;

        pub word -> Symbol = w:ident -> {
            _state.user().n += 1;
            w
        }
    }
}

#[test]
fn one_state_carries_both_declarations() {
    Both::parse_word()
        .parse_test_in(App::default(), "hello")
        .assert_success_with(|s, ctx| {
            assert_eq!(ctx.user_state.slots.names, ["hello"]);
            assert_eq!(ctx.user_state.counted.n, 1);
            assert_eq!(s.index(), 0);
        });
}

// A grammar that declares nothing keeps the built-in interner and its cache.
grammar! {
    grammar Default_ {
        pub two -> (Symbol, Symbol) = a:ident b:ident -> { (a, b) }
    }
}

#[test]
fn a_grammar_without_a_declaration_is_unchanged() {
    Default_::parse_two()
        .parse_test("alpha alpha")
        .assert_success_with(|(a, b), ctx| {
            assert_eq!(a, b);
            assert_eq!(ctx.interner.resolve(*a), "alpha");
            assert_eq!(ctx.interner.len(), 1);
        });
}

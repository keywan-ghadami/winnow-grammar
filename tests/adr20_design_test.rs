//! The feasibility claims of ADR 20, compiled.
//!
//! ADR 20 proposes `state T;` - a grammar naming the user state it needs. Its
//! design turns on three questions that are cheaper to answer with a compiler
//! than with an argument, so they are answered here and the ADR cites this
//! file. Nothing in it uses a feature that exists yet: these are the shapes the
//! generated code would take, written by hand.
//!
//! 1. Can `state T` be a **bound** rather than a substitution - so that rules
//!    stay generic and two grammars with different states compose?
//! 2. Does the blanket impl that makes a bare `T` satisfy its own bound
//!    collide with a composite state's impls? (Coherence says no, but coherence
//!    is easier to run than to reason about.)
//! 3. Can a state-taking test helper live beside `parse_test`, which is fixed
//!    to `ParseContext<()>` so that tests need no turbofish?

/// What the crate would provide.
pub trait StateOf<T> {
    fn state(&mut self) -> &mut T;
}

/// The blanket: a state that *is* the table satisfies it with no impl of the
/// user's own.
impl<T> StateOf<T> for T {
    fn state(&mut self) -> &mut T {
        self
    }
}

#[derive(Clone, Debug, Default)]
struct TableA {
    n: usize,
}
#[derive(Clone, Debug, Default)]
struct TableB {
    m: usize,
}

/// A composite state, so that two pinned grammars can run over one context.
#[derive(Clone, Debug, Default)]
struct App {
    a: TableA,
    b: TableB,
}

// The question: do these overlap with the blanket impl?
impl StateOf<TableA> for App {
    fn state(&mut self) -> &mut TableA {
        &mut self.a
    }
}
impl StateOf<TableB> for App {
    fn state(&mut self) -> &mut TableB {
        &mut self.b
    }
}

// What a generated rule of a grammar with `state TableA;` would look like.
fn rule_a<S: StateOf<TableA>>(s: &mut S) -> usize {
    let t: &mut TableA = s.state();
    t.n += 1;
    t.n
}

fn rule_b<S: StateOf<TableB>>(s: &mut S) -> usize {
    let t: &mut TableB = s.state();
    t.m += 2;
    t.m
}

#[test]
fn a_bare_table_satisfies_its_own_bound() {
    let mut t = TableA::default();
    assert_eq!(rule_a(&mut t), 1);
}

#[test]
fn one_composite_state_serves_two_pinned_grammars() {
    let mut app = App::default();
    assert_eq!(rule_a(&mut app), 1);
    assert_eq!(rule_b(&mut app), 2);
    assert_eq!(rule_a(&mut app), 2);
    assert_eq!(app.a.n, 2);
    assert_eq!(app.b.m, 2);
}

// -----------------------------------------------------------------------------
// Cost 2: can `parse_test` (fixed to `()`) and a state-taking sibling coexist
// without making either call ambiguous?
// -----------------------------------------------------------------------------

use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::testing::WinnowTestExt;
use winnow_grammar::{grammar, ParseContext, ParseInput};

/// The sibling: blanket over every state, so the state argument determines it.
pub trait WinnowTestExtWith<'a, O, S> {
    fn parse_test_in(&mut self, state: S, input: &'a str) -> Result<O, String>;
}

impl<'a, P, O, S> WinnowTestExtWith<'a, O, S> for P
where
    P: Parser<ParseInput<'a, S>, O, winnow_grammar::error::ParseError>,
    S: Clone + std::fmt::Debug,
    O: std::fmt::Debug,
{
    fn parse_test_in(&mut self, state: S, input: &'a str) -> Result<O, String> {
        // Cost 3, and its mitigation: `..Default::default()` would require
        // `S: Default`, which a state carrying a pre-sized table need not be.
        // Naming every field instead needs nothing of `S` - which is what a
        // `ParseContext::with_state(user_state)` constructor would wrap up.
        let mut stream = ParseInput {
            input: LocatingSlice::new(input),
            state: ParseContext {
                interner: winnow_grammar::InternerContext::new(),
                user_state: state,
                furthest: None,
                rules: Vec::new(),
                diagnose: Default::default(),
                fold: Default::default(),
            },
        };
        self.parse_next(&mut stream).map_err(|e| e.render(input))
    }
}

grammar! {
    grammar Probe {
        pub n -> u32 = v:u32 -> { v }
    }
}

#[test]
fn the_existing_helper_still_needs_no_turbofish() {
    Probe::parse_n().parse_test("42").assert_success_is(42);
}

#[test]
fn the_sibling_takes_the_state_and_infers_from_it() {
    assert_eq!(
        Probe::parse_n().parse_test_in(TableA::default(), "42"),
        Ok(42)
    );
    assert_eq!(Probe::parse_n().parse_test_in(App::default(), "7"), Ok(7));
}

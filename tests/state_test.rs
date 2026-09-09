//! `state T;` - the state a grammar declares it needs (ADR 20).
//!
//! The declaration is a *bound*, not a substitution: rules stay generic over
//! the state and require only that it provides a `T`. That is what lets one
//! state serve two grammars, and it is what these tests pin.

use winnow::token::take_till;
use winnow::Parser;
use winnow_grammar::error::ParseError;
use winnow_grammar::testing::{WinnowTestExt, WinnowTestExtWith};
use winnow_grammar::{grammar, ParseContext, ParseInput, StateOf};

// -----------------------------------------------------------------------------
// An action reaches the declared state through `_state.user()`.
// -----------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
struct Seen {
    words: usize,
    letters: usize,
}

grammar! {
    grammar Counting {
        state Seen;

        pub word -> usize = w:alpha1 -> {
            _state.user().words += 1;
            _state.user().letters += w.len();
            w.len()
        }

        pub words -> usize = ws:word* -> { ws.iter().sum() }
    }
}

#[test]
fn an_action_writes_to_the_declared_state() {
    Counting::parse_words()
        .parse_test_in(Seen::default(), "alpha beta gamma")
        .assert_success_with(|total, ctx| {
            assert_eq!(*total, 14);
            assert_eq!(
                ctx.user_state,
                Seen {
                    words: 3,
                    letters: 14
                }
            );
        });
}

#[test]
fn the_context_and_the_state_are_reachable_from_one_action() {
    // `_state.user()` is a method, not a second binding, so it does not hold a
    // borrow across the rest of the action - both can be used in either order.
    grammar! {
        grammar Both {
            state Seen;
            pub w -> winnow_grammar::Symbol = s:alpha1 -> {
                _state.user().words += 1;
                let sym = _state.interner.intern_string(s);
                _state.user().letters += s.len();
                sym
            }
        }
    }

    Both::parse_w()
        .parse_test_in(Seen::default(), "hello")
        .assert_success_with(|sym, ctx| {
            assert_eq!(ctx.interner.resolve(*sym), "hello");
            assert_eq!(ctx.user_state.words, 1);
            assert_eq!(ctx.user_state.letters, 5);
        });
}

// -----------------------------------------------------------------------------
// One state, two grammars: the reason `state` is a bound.
// -----------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct Names {
    n: usize,
}

grammar! {
    grammar CountsNames {
        state Names;
        pub name -> usize = _n:alpha1 -> { _state.user().n += 1; _state.user().n }
    }
}

#[derive(Clone, Debug, Default)]
struct App {
    seen: Seen,
    names: Names,
}

impl StateOf<Seen> for App {
    fn state(&mut self) -> &mut Seen {
        &mut self.seen
    }
}

impl StateOf<Names> for App {
    fn state(&mut self) -> &mut Names {
        &mut self.names
    }
}

#[test]
fn one_composite_state_serves_two_grammars() {
    // Two grammars, each declaring a different state, parsing through one
    // context. A substitution-based `state` could not express this at all.
    let mut stream = ParseInput {
        input: winnow::stream::LocatingSlice::new("alpha"),
        state: ParseContext::with_state(App::default()),
    };
    assert_eq!(Counting::parse_word().parse_next(&mut stream).unwrap(), 5);

    let mut stream2 = ParseInput {
        input: winnow::stream::LocatingSlice::new("beta"),
        state: stream.state,
    };
    assert_eq!(
        CountsNames::parse_name().parse_next(&mut stream2).unwrap(),
        1
    );

    let app = stream2.state.user_state;
    assert_eq!(app.seen.words, 1);
    assert_eq!(app.names.n, 1);
}

// -----------------------------------------------------------------------------
// The high-end shape: a hand-written parser that assigns slots out of the
// declared state, and a fold that aggregates by slot - ADR 20.
// -----------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct Slots {
    names: Vec<String>,
}

impl Slots {
    /// The identity *is* the slot: the number indexes the caller's own table.
    fn slot(&mut self, name: &str) -> usize {
        if let Some(i) = self.names.iter().position(|n| n == name) {
            return i;
        }
        self.names.push(name.to_string());
        self.names.len() - 1
    }
}

fn city<'a, S>(i: &mut ParseInput<'a, S>) -> Result<usize, ParseError>
where
    S: Clone + std::fmt::Debug + StateOf<Slots>,
{
    let name: &str = take_till(1.., ';').parse_next(i)?;
    Ok(i.state.user_state.state().slot(name))
}

grammar! {
    grammar Measurements {
        state Slots;

        WS -> () = "" -> { () }

        extern rule city -> usize;

        #[frame(boundary = "\n")]
        pub ROW -> (usize, i32) = c:city ";" t:i32 frame_end -> { (c, t) }

        pub FILE -> Vec<i64> = totals:par_fold(
            ROW,
            Vec::new,
            |mut acc: Vec<i64>, (slot, temp): (usize, i32)| {
                if slot >= acc.len() { acc.resize(slot + 1, 0); }
                acc[slot] += temp as i64;
                acc
            },
            |a: Vec<i64>, b: Vec<i64>| {
                let (mut long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
                for (i, v) in short.into_iter().enumerate() { long[i] += v; }
                long
            }
        ) -> { totals }
    }
}

#[test]
fn a_hand_written_parser_reaches_the_declared_state() {
    let input = "Hamburg;12\nZürich;20\nHamburg;-4\n東京;30\n";
    Measurements::parse_FILE()
        .parse_test_in(Slots::default(), input)
        .assert_success_with(|totals, ctx| {
            let by = |name: &str| {
                let i = ctx.user_state.names.iter().position(|n| n == name).unwrap();
                totals[i]
            };
            assert_eq!(by("Hamburg"), 8);
            assert_eq!(by("Zürich"), 20);
            assert_eq!(by("東京"), 30);
            assert_eq!(ctx.user_state.names.len(), 3);
        });
}

#[test]
fn a_declared_state_still_cuts_into_pieces() {
    // A table that must start empty in every piece is what
    // `parse_…_pieces_with` is for (ADR 19 §2) - and for this workload it is
    // the ordinary entry point, not an exception. Slot numbers are then per
    // piece, exactly as symbols are per interner, so what merges here is the
    // count, not the identity.
    use winnow_grammar::rt::Parallelism;
    let input = "Hamburg;12\nZürich;20\nHamburg;-4\n東京;30\n";
    let totals = Measurements::parse_FILE_pieces_with(
        input,
        || ParseContext::with_state(Slots::default()),
        Parallelism::Pieces(2),
    )
    .unwrap();
    assert_eq!(totals.iter().sum::<i64>(), 58);
}

// A grammar that declares nothing keeps working exactly as before - the
// feature is additive.
grammar! {
    grammar Unchanged {
        pub n -> u32 = v:u32 -> { v }
    }
}

#[test]
fn a_grammar_without_a_state_is_unchanged() {
    Unchanged::parse_n().parse_test("42").assert_success_is(42);
}

//! `parse_<rule>_slice` runs its passes as the parse in memory runs them
//! (ADR 17): a fast pass, and only when that fails a diagnosing one - under
//! `Replay` on the user state from before the fast pass, under
//! `ReplayInPlace` on the state it left. So what an action did to the user
//! state comes out as it does in memory, success or failure, in every
//! `Diagnose` mode; and a cut that stays open across the fold costs a
//! successful parse no second pass.

use std::ops::ControlFlow;
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::{grammar, Diagnose, ParseContext, ParseInput};

#[derive(Clone, Debug, Default, PartialEq)]
struct Tally {
    items: usize,
    heads: usize,
}

grammar! {
    grammar Count {
        state Tally;

        #[stream(each)]
        pub doc -> usize = h:head "=>" n:fold(num, || 0usize, |n: usize, _v: i64| n + 1) "]" -> { h + n }
        head -> usize = "[" -> { _state.user().heads += 1; 0 }
        num -> i64 = v:i64 ","? -> { _state.user().items += 1; v }
    }
}

const MODES: [Diagnose; 4] = [
    Diagnose::Off,
    Diagnose::Replay,
    Diagnose::ReplayInPlace,
    Diagnose::Eager,
];

fn ctx(mode: Diagnose) -> ParseContext<Tally> {
    ParseContext {
        diagnose: mode,
        ..Default::default()
    }
}

fn in_memory(input: &str, mode: Diagnose) -> (Result<usize, String>, Tally) {
    let mut i = ParseInput {
        input: LocatingSlice::new(input),
        state: ctx(mode),
    };
    let r = Count::parse_doc().parse_next(&mut i);
    (r.map_err(|e| e.to_string()), i.state.user_state)
}

const INPUTS: [&str; 5] = [
    "[ => 1, 2, 3, 4, 5 ]",
    "[ => ]",
    "[ => 1, 2, x, 4 ]",
    "[ => 1, 2, 3",
    "[ 1, 2 ]",
];

#[test]
fn the_value_the_error_and_the_user_state_are_the_parse_in_memorys() {
    for input in INPUTS {
        for mode in MODES {
            let (want, want_state) = in_memory(input, mode);
            let mut c = ctx(mode);
            let got = Count::parse_doc_slice(input.as_bytes(), &mut c).map_err(|e| e.to_string());
            assert_eq!(got, want, "{input:?} {mode:?}");
            assert_eq!(c.user_state, want_state, "{input:?} {mode:?}");
            assert_eq!(
                c.diagnose, mode,
                "{input:?}: the mode is the caller's again"
            );
        }
    }
}

#[test]
fn each_sees_the_items_before_an_error_once() {
    for mode in MODES {
        let mut seen = Vec::new();
        let r = Count::parse_doc_slice_each(b"[ => 1, 2, x, 4 ]", &mut ctx(mode), |v: &i64| {
            seen.push(*v);
            ControlFlow::<()>::Continue(())
        });
        let (want, _) = in_memory("[ => 1, 2, x, 4 ]", mode);
        assert_eq!(
            r.map_err(|e| e.to_string()).map(|_| 0),
            want.map(|_| 0),
            "{mode:?}"
        );
        // The diagnosing pass parses the items again; `each` saw each once.
        assert_eq!(seen, [1, 2], "{mode:?}");
    }
}

#[test]
fn each_and_the_user_state_agree_on_success() {
    for mode in MODES {
        let mut seen = Vec::new();
        let mut c = ctx(mode);
        let r = Count::parse_doc_slice_each(b"[ => 1, 2, 3 ]", &mut c, |v: &i64| {
            seen.push(*v);
            ControlFlow::<()>::Continue(())
        });
        assert_eq!(r.unwrap(), ControlFlow::Continue(3), "{mode:?}");
        assert_eq!(seen, [1, 2, 3], "{mode:?}");
        assert_eq!(c.user_state, Tally { items: 3, heads: 1 }, "{mode:?}");
    }
}

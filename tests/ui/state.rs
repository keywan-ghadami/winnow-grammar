//! What `state T;` rejects - ADR 20.

use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExtWith;

#[derive(Clone, Debug, Default)]
pub struct Table {
    pub n: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Unrelated;

grammar! {
    grammar Needs {
        state Table;
        pub r -> usize = _w:alpha1 -> { _state.user().n += 1; _state.user().n }
    }
}

// A grammar declares at most one state.
grammar! {
    grammar Twice {
        state Table;
        state Unrelated;
        pub r -> usize = _w:alpha1 -> { 0 }
    }
}

fn main() {
    // The state does not provide what the grammar declared.
    let _ = Needs::parse_r().parse_test_in(Unrelated, "abc");
}

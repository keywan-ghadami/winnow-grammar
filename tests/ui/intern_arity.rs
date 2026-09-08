//! `intern` takes exactly one pattern - ADR 18 §1.

use winnow_grammar::{grammar, Symbol};

grammar! {
    grammar NoArgument {
        pub r -> Symbol = s:intern() -> { s }
    }
}

grammar! {
    grammar TwoArguments {
        pub r -> Symbol = s:intern(alpha1, digit1) -> { s }
    }
}

fn main() {}

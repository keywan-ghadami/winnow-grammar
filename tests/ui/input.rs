//! What `input bytes;` rejects - ADR 25.

use winnow_grammar::grammar;

// A grammar declares its input at most once.
grammar! {
    grammar Twice {
        input bytes;
        input bytes;
        pub r -> () = "a" -> { () }
    }
}

fn main() {}

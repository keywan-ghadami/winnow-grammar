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

// `bytes(p)` exists only in a byte grammar.
grammar! {
    grammar TextOnly {
        pub r -> usize = b:bytes(digit1) -> { b.len() }
    }
}

fn main() {}

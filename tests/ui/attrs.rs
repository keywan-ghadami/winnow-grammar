//! An attribute nothing reads is a mistake, not a decoration: a typo in
//! `#[frame]` would otherwise leave a rule quietly not a frame.

use winnow_grammar::grammar;

grammar! {
    grammar Typo {
        #[frmae(boundary = "\n")]
        pub ROW -> usize = d:digit1 "\n" -> { d.len() }
    }
}

grammar! {
    grammar Decoration {
        #[allow(dead_code)]
        pub A -> usize = d:digit1 -> { d.len() }
    }
}

// The declaration routes to `ExternRule` even behind an attribute, so the
// attribute check reaches it too.
grammar! {
    grammar OnExtern {
        #[frmae(boundary = "\n")]
        extern rule outside -> usize;

        pub B -> usize = d:digit1 -> { d.len() }
    }
}

fn main() {}

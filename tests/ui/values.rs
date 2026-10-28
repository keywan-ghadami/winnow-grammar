//! What values in the grammar reject - ADR 26.

use winnow_grammar::grammar;

// A value goes to a typed parameter, not to one that takes a parser.
grammar! {
    grammar ToParser {
        pub R -> usize = m:count(" ") v:WRAP(p = m) -> { v }
        WRAP<T>(p: Rule<T>) -> T = v:p -> { v }
    }
}

// A guard matches nothing, so there is nothing to bind.
grammar! {
    grammar BoundGuard {
        pub R -> () = g:if(true) "a" -> { () }
    }
}

// A built-in that wraps a pattern takes no value.
grammar! {
    grammar BuiltinValue {
        pub R(n: usize) -> &'a str = t:text(n) -> { t }
    }
}

fn main() {}

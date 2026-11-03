//! Where a lookahead's value may be bound: outside it, `name:peek(p)`.

use winnow_grammar::grammar;

// A binding inside `peek(…)` names a value the generated code never has.
grammar! {
    grammar Inside {
        pub R -> &'a str = peek(a:raw_ident) b:raw_ident -> { b }
    }
}

// `not(…)` succeeds only when nothing matched: there is nothing to bind.
grammar! {
    grammar BoundNot {
        pub R -> () = n:not("a") "b" -> { () }
    }
}

// Nor inside it.
grammar! {
    grammar InsideNot {
        pub R -> () = not(a:"a") "b" -> { () }
    }
}

fn main() {}

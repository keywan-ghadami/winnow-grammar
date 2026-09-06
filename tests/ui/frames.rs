//! What the frame check rejects, and why each is a compile error rather than
//! a data-dependent wrong answer at runtime.

use winnow_grammar::grammar;

// A literal inside the frame contains the boundary: the CSV-with-quoted-
// newlines case. Cutting at the next "\n" could land inside a record.
grammar! {
    grammar LiteralInside {
        #[frame]
        RECORD -> () = "key" ":" "\n" "value" "\n" -> { () }
    }
}

// A syntactic rule: the implicit whitespace between its elements is
// `multispace0`, which eats newlines.
grammar! {
    grammar SyntacticFrame {
        #[frame]
        record -> () = "a" ";" "\n" -> { () }
    }
}

// `any` can be anything, the boundary included.
grammar! {
    grammar AnyInside {
        #[frame]
        RECORD -> () = any "\n" -> { () }
    }
}

// Reached from the frame: the problem is two rules away, and the message
// names the rule it is in.
grammar! {
    grammar ReachedFromFrame {
        VALUE -> () = "x" "\n" "y" -> { () }
        ITEM -> () = VALUE -> { () }
        #[frame]
        RECORD -> () = ITEM "\n" -> { () }
    }
}

// The rule does not end in its boundary.
grammar! {
    grammar NoTerminator {
        #[frame = "\n"]
        RECORD -> () = "\n" "a" -> { () }
    }
}

// No trailing literal to infer a boundary from.
grammar! {
    grammar NoInference {
        #[frame]
        RECORD -> () = digit1 line_ending -> { () }
    }
}

// par_fold over a rule that is not a frame.
grammar! {
    grammar NotAFrame {
        ITEM -> i32 = v:i32 "\n" -> { v }
        pub FILE -> i32 = par_fold(ITEM, || 0, |a: i32, b: i32| a + b, |a: i32, b: i32| a + b)
    }
}

// par_fold with something before it in the sequence.
grammar! {
    grammar NotWholeBody {
        #[frame]
        ITEM -> i32 = v:i32 "\n" -> { v }
        pub FILE -> i32 = "header\n" t:par_fold(ITEM, || 0, |a: i32, b: i32| a + b, |a: i32, b: i32| a + b) -> { t }
    }
}

// par_fold without its merge.
grammar! {
    grammar NoMerge {
        #[frame]
        ITEM -> i32 = v:i32 "\n" -> { v }
        pub FILE -> i32 = par_fold(ITEM, || 0, |a: i32, b: i32| a + b)
    }
}

// A malformed attribute.
grammar! {
    grammar BadAttr {
        #[frame = 3]
        RECORD -> () = "a" "\n" -> { () }
    }
}

fn main() {}

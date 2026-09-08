//! What the frame check rejects, and why each is a compile error rather than
//! a data-dependent wrong answer at runtime. The messages are the contract:
//! each names the construct and, where there is one, the thing to write.

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
        #[frame(boundary = "\n")]
        RECORD -> () = "\n" "a" -> { () }
    }
}

// `until` whose terminator does not cover the boundary: the skip would run
// through it. The message says what to add.
grammar! {
    grammar UncoveredUntil {
        NAME -> &'a str = s:until(";") -> { s }
        #[frame]
        RECORD -> usize = n:NAME ";" "\n" -> { n.len() }
    }
}

// `recover` under a frame: the skip and the synchronization token cannot
// both be kept off the boundary.
grammar! {
    grammar RecoverInside {
        ITEM -> i32 = v:i32 -> { v }
        #[frame]
        RECORD -> Option<i32> = v:recover(ITEM, ";") ";" "\n" -> { v }
    }
}

// `frame_end` where no frame reaches the rule: nothing for it to stand for.
grammar! {
    grammar FrameEndOutside {
        pub NAME -> &'a str = s:until(";" | frame_end) -> { s }
    }
}

// A rule that writes `frame_end` and is reached from two frames with
// different boundaries: it would have to mean two things.
grammar! {
    grammar TwoBoundaries {
        NAME -> &'a str = s:until(";" | frame_end) -> { s }
        #[frame(boundary = "\n")]
        LINE -> usize = n:NAME ";" frame_end -> { n.len() }
        #[frame(boundary = "|")]
        CELL -> usize = n:NAME ";" frame_end -> { n.len() }
    }
}

// The positional attribute forms are not forms: the keyed one is.
grammar! {
    grammar PositionalNameValue {
        #[frame = "\n"]
        RECORD -> () = "a" "\n" -> { () }
    }
}

grammar! {
    grammar PositionalList {
        #[frame("\n")]
        RECORD -> () = "a" "\n" -> { () }
    }
}

grammar! {
    grammar UnknownKey {
        #[frame(separator = "\n")]
        RECORD -> () = "a" "\n" -> { () }
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

// A malformed boundary.
grammar! {
    grammar BadAttr {
        #[frame(boundary = 3)]
        RECORD -> () = "a" "\n" -> { () }
    }
}

// `intern` is transparent to the frame check, so an argument that runs
// through the boundary is still rejected - at the argument.
grammar! {
    grammar InternedUntilRunsThrough {
        #[frame(boundary = "\n")]
        ROW -> winnow_grammar::Symbol = c:intern(until(";")) ";" "\n" -> { c }
    }
}

fn main() {}

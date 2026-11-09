//! A fold's named arguments `sep`, `min` and `max` are checked where they are
//! written.

use winnow_grammar::grammar;

grammar! {
    grammar MaxZero {
        pub r -> usize = n:fold(item, || 0usize, |n: usize, _x: ()| n + 1, max = 0) -> { n }
        item -> () = "x" -> { () }
    }
}

grammar! {
    grammar MinAboveMax {
        pub r -> usize = n:fold(item, || 0usize, |n: usize, _x: ()| n + 1, min = 3, max = 2) -> { n }
        item -> () = "x" -> { () }
    }
}

grammar! {
    grammar Twice {
        pub r -> usize = n:fold(item, || 0usize, |n: usize, _x: ()| n + 1, min = 1, min = 2) -> { n }
        item -> () = "x" -> { () }
    }
}

grammar! {
    grammar Unknown {
        pub r -> usize = n:fold(item, || 0usize, |n: usize, _x: ()| n + 1, step = 2) -> { n }
        item -> () = "x" -> { () }
    }
}

grammar! {
    grammar Parallel {
        pub FILE -> i32 = par_fold(ITEM, || 0, |a: i32, b: i32| a + b, |a: i32, b: i32| a + b, sep = ",")
        ITEM -> i32 = "x\n" -> { 1 }
    }
}

fn main() {}

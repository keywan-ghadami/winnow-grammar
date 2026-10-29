// What `#[stream]` refuses, and what it says (ADR 27 §3).
use winnow_grammar::grammar;

// Two alternatives on the path: an item handed out could not be taken back.
grammar! {
    grammar TwoAlternatives {
        #[stream]
        pub list -> usize = "[" n:fold(item, || 0usize, |n: usize, _x: ()| n + 1) "]" -> { n }
                          | "()" -> { 0 }
        item -> () = "x" -> { () }
    }
}

// No fold anywhere below.
grammar! {
    grammar NoFold {
        #[stream]
        pub list -> usize = "[" "]" -> { 0 }
    }
}

// Two elements that lead to a fold.
grammar! {
    grammar TwoFolds {
        #[stream]
        pub list -> usize =
            a:fold(item, || 0usize, |n: usize, _x: ()| n + 1)
            b:fold(item, || 0usize, |n: usize, _x: ()| n + 1)
            -> { a + b }
        item -> () = "x" -> { () }
    }
}

// A value kept across a stop whose type is not known.
grammar! {
    grammar UntypedValue {
        #[stream]
        pub list -> usize = h:"head" n:fold(item, || 0usize, |n: usize, _x: ()| n + 1) -> { n + h.len() }
        item -> () = "x" -> { () }
    }
}

// A value kept across a stop that borrows the input.
grammar! {
    grammar BorrowedValue {
        #[stream]
        pub list -> usize = h:head n:fold(item, || 0usize, |n: usize, _x: ()| n + 1) -> { n + h.len() }
        head -> &'a str = h:raw_ident -> { h }
        item -> () = "x" -> { () }
    }
}

// The fold's value used before the action.
grammar! {
    grammar FoldValueInSuffix {
        #[stream]
        pub list -> usize = n:fold(item, || 0usize, |n: usize, _x: ()| n + 1) if(n > 0) -> { n }
        item -> () = "x" -> { () }
    }
}

// The item is not a call of a rule.
grammar! {
    grammar ItemNotARule {
        #[stream]
        pub list -> usize = n:fold("x", || 0usize, |n: usize, _x: &str| n + 1) -> { n }
    }
}

// A rule that returns a view of the input.
grammar! {
    grammar BorrowedResult {
        #[stream]
        pub list -> &'a str = "[" fold(item, || 0usize, |n: usize, _x: ()| n + 1) "]" -> { "" }
        item -> () = "x" -> { () }
    }
}

// Arguments to the attribute.
grammar! {
    grammar WithArguments {
        #[stream(item)]
        pub list -> usize = n:fold(item, || 0usize, |n: usize, _x: ()| n + 1) -> { n }
        item -> () = "x" -> { () }
    }
}

fn main() {}

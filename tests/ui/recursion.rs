use winnow_grammar::grammar;

// Test case: Indirect Left Recursion (Should Fail Compile)
grammar! {
    grammar IndirectRec {
        rule a -> () = b -> { () }
        rule b -> () = a -> { () }
    }
}

fn main() {}

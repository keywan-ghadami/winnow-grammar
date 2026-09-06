//! Malformed repetition bounds are rejected at the bound, with a message that
//! says what the shapes are.

use winnow_grammar::grammar;

grammar! {
    grammar UpperBelowLower {
        pub r -> usize = d:digit{3,1} -> { d.len() }
    }
}

grammar! {
    grammar MatchesNothing {
        pub r -> usize = d:digit{0} -> { d.len() }
    }
}

grammar! {
    grammar ZeroUpperBound {
        pub r -> usize = d:digit{0,0} -> { d.len() }
    }
}

grammar! {
    grammar TooManyParts {
        pub r -> usize = d:digit{1,2,3} -> { d.len() }
    }
}

fn main() {}

// A `par_fold` rule whose result borrows the input has no `_reader`: a block
// lives for one step, and what the fold returns would outlive it (ADR 27 §1).
use winnow_grammar::grammar;

grammar! {
    grammar Borrowing {
        #[frame(boundary = "\n")]
        pub ROW -> &'a str = s:until(frame_end) frame_end -> { s }
        pub FILE -> Vec<&'a str> = v:par_fold(
            ROW,
            Vec::new,
            |mut acc: Vec<&'a str>, s: &'a str| { acc.push(s); acc },
            |mut a: Vec<&'a str>, b: Vec<&'a str>| { a.extend(b); a }
        ) -> { v }
    }
}

fn main() {
    let _ = Borrowing::parse_FILE_reader(
        std::io::empty(),
        &winnow_grammar::ParseContext::<()>::default(),
        winnow_grammar::rt::Parallelism::Off,
        winnow_grammar::rt::Reading::default(),
    );
}

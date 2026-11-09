// A rule's visibility is given to every function generated for it. A
// `pub(crate)` rule's functions, `_reader` and `_reader_each` included, cannot
// leave the crate; a `pub(super)` rule reaches the module that holds
// `grammar!`, not further.
mod outer {
    pub mod inner {
        winnow_grammar::grammar! {
            grammar G {
                #[stream]
                pub(crate) doc -> usize =
                    n:fold(item, || 0usize, |n: usize, _i: i64| n + 1) eof -> { n }

                #[stream(each)]
                pub(crate) docs -> usize =
                    n:fold(item, || 0usize, |n: usize, _i: i64| n + 1) eof -> { n }

                pub(super) near -> i64 = i:item -> { i }

                item -> i64 = n:i64 -> { n }
            }
        }

        // Inside `inner`, the `pub(super)` rule is in reach.
        pub fn near_is_here() {
            let _ = G::parse_near::<()>;
        }
    }
}

#[allow(unused_imports)]
pub use outer::inner::G::{parse_doc, parse_doc_reader, parse_docs_reader_each};

fn main() {
    outer::inner::near_is_here();
    let _ = outer::inner::G::parse_near::<()>;
}

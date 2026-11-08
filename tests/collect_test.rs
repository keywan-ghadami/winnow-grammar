//! `collect L;` - the collection a grammar's bound repetitions collect into -
//! and generated code that names the standard library by full path, so that a
//! crate with its own `Vec`, `String`, `Option`, `Result` or `Box` in scope
//! still expands.

use winnow_grammar::testing::WinnowTestExt;

/// A list that is not `Vec`: what a grammar names with `collect List;`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct List<T>(pub ::std::vec::Vec<T>);

impl<T> winnow_grammar::Collect<T> for List<T> {
    fn empty() -> Self {
        List(::std::vec::Vec::new())
    }
    fn push(&mut self, item: T) {
        self.0.push(item)
    }
    fn len(&self) -> usize {
        self.0.len()
    }
}

/// A crate whose own names hide the prelude's - as Nikaia's programs import
/// their own `Vec` and `String` into every file. The grammars below are
/// expanded here, where `use super::*` brings these into the generated module.
#[allow(dead_code)]
mod shadowed {
    use winnow_grammar::grammar;

    pub struct Vec<T>(T);
    pub struct String;
    pub struct Option<T>(T);
    pub struct Result<T, E>(T, E);
    pub struct Box<T>(T);
    // And the variants the generated code builds and matches.
    pub struct Some;
    pub struct None;
    pub struct Ok;
    pub struct Err;

    pub use super::List;

    #[derive(Debug, Clone, PartialEq)]
    pub enum Expr {
        Num(i64),
        Add(::std::boxed::Box<Expr>, ::std::boxed::Box<Expr>),
    }

    // The default collection, with as many kinds of rule as fit: a bound
    // repetition, an option, alternatives with and without actions, left
    // recursion, a span, a fold, a count, a run of characters, a bounded
    // repetition, a recovery and a label.
    grammar! {
        grammar Plain {
            pub rule items -> ::std::vec::Vec<i64> = xs:item* -> { xs }

            rule item -> i64 = n:i64 ","? -> { n }

            pub rule expr -> Expr =
                l:expr "+" r:term -> { Expr::Add(::std::boxed::Box::new(l), ::std::boxed::Box::new(r)) }
              | t:term -> { t }

            pub rule term -> Expr # "a number" = n:i64 -> { Expr::Num(n) }

            pub rule maybe -> ::core::option::Option<i64> = "(" n:i64? ")" -> { n }

            pub rule word -> (::std::string::String, ::core::ops::Range<usize>) =
                w:raw_ident @ s -> { (w.to_string(), s) }

            pub rule total -> i64 =
                s:fold(item, || 0i64, |acc: i64, n: i64| acc + n) -> { s }

            pub rule how_many -> usize = n:count(item) -> { n }

            pub DIGITS -> &'a str = d:digit+ -> { d }

            pub rule pair -> ::std::vec::Vec<i64> = xs:item{1,2} -> { xs }

            pub rule kept -> ::std::vec::Vec<::core::option::Option<i64>> =
                xs:recover(semi, ";")* -> { xs }

            rule semi -> i64 = n:i64 ";" -> { n }

            pub rule bit -> bool = "1" -> { true } | "0" -> { false }
        }
    }

    // The same repetitions, collected into `List` instead of `Vec`.
    grammar! {
        grammar Listed {
            collect List;

            pub rule items -> List<i64> = xs:item* -> { xs }

            rule item -> i64 = n:i64 ","? -> { n }

            pub rule some -> List<i64> = xs:item+ -> { xs }

            pub rule pair -> List<i64> = xs:item{2} -> { xs }

            pub rule nested -> List<List<i64>> = xss:group* -> { xss }

            rule group -> List<i64> = "[" xs:item* "]" -> { xs }

            // A run of characters is still the text it matched.
            pub DIGITS -> &'a str = d:digit+ -> { d }
        }
    }

    // A path, not only a name.
    grammar! {
        grammar ByPath {
            collect super::List;

            pub rule items -> super::List<i64> = xs:n* -> { xs }

            rule n -> i64 = v:i64 -> { v }
        }
    }
}

use shadowed::{ByPath, Expr, Listed, Plain};

fn list<T>(items: impl IntoIterator<Item = T>) -> List<T> {
    List(items.into_iter().collect())
}

#[test]
fn a_grammar_expands_where_the_prelude_is_hidden() {
    Plain::parse_items()
        .parse_test("1, 2, 3")
        .assert_success_is(vec![1, 2, 3]);
    Plain::parse_expr()
        .parse_test("1 + 2")
        .assert_success_is(Expr::Add(Box::new(Expr::Num(1)), Box::new(Expr::Num(2))));
    Plain::parse_maybe()
        .parse_test("( )")
        .assert_success_is(None);
    Plain::parse_maybe()
        .parse_test("(7)")
        .assert_success_is(Some(7));
    Plain::parse_word()
        .parse_test("  abc")
        .assert_success_is(("abc".to_string(), 2..5));
    Plain::parse_total()
        .parse_test("1, 2, 39")
        .assert_success_is(42);
    Plain::parse_how_many()
        .parse_test("4, 5")
        .assert_success_is(2);
    Plain::parse_DIGITS()
        .parse_test("0815")
        .assert_success_is("0815");
    Plain::parse_pair()
        .parse_test("8, 9")
        .assert_success_is(vec![8, 9]);
    Plain::parse_kept()
        .parse_test("1; x; 3;")
        .assert_success_is(vec![Some(1), None, Some(3)]);
    Plain::parse_bit().parse_test("0").assert_success_is(false);
    Plain::parse_term().parse_test("x").assert_failure();
}

#[test]
fn repetitions_collect_into_the_declared_list() {
    Listed::parse_items()
        .parse_test("1, 2, 3")
        .assert_success_is(list([1, 2, 3]));
    Listed::parse_items()
        .parse_test("")
        .assert_success_is(list([]));
    Listed::parse_some()
        .parse_test("5")
        .assert_success_is(list([5]));
    Listed::parse_some().parse_test("").assert_failure();
    Listed::parse_pair()
        .parse_test("1, 2")
        .assert_success_is(list([1, 2]));
    Listed::parse_pair().parse_test("1").assert_failure();
    Listed::parse_nested()
        .parse_test("[1, 2] [] [3]")
        .assert_success_is(list([list([1, 2]), list([]), list([3])]));
    Listed::parse_DIGITS()
        .parse_test("42")
        .assert_success_is("42");
    ByPath::parse_items()
        .parse_test("4 5 6")
        .assert_success_is(list([4, 5, 6]));
}

#[test]
fn an_error_in_a_list_numbers_its_item() {
    // `len` is what numbers the element that failed, as `Vec`'s did.
    let e = Listed::parse_pair().parse_test("1, x").assert_failure();
    assert!(e.to_string().contains("item 2"), "{e}");
}

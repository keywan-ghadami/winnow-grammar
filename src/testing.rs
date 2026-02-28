use winnow::error::ContextError;
use winnow::stream::LocatingSlice;
use winnow::Parser;

pub use grammar_kit::testing::*;

/// Extension trait for winnow parsers to simplify testing.
///
/// This trait allows writing tests similar to `syn::parse::Parser::parse_str`.
/// It handles the creation of `LocatingSlice` and conversion of results into `TestResult`.
pub trait WinnowTestExt<'a, O> {
    fn parse_test(&mut self, input: &'a str) -> TestResult<O, String>;
}

impl<'a, P, O> WinnowTestExt<'a, O> for P
where
    P: Parser<LocatingSlice<&'a str>, O, ContextError>,
    O: std::fmt::Debug,
{
    fn parse_test(&mut self, input: &'a str) -> TestResult<O, String> {
        let stream = LocatingSlice::new(input);
        match self.parse(stream) {
            Ok(val) => TestResult::new(Ok(val)).with_source(input),
            Err(e) => {
                // formatting the error simple for now
                let msg = format!("{}", e);
                TestResult::new(Err(msg)).with_source(input)
            }
        }
    }
}

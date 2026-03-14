use winnow::prelude::*;

fn parse_value_inner<'a, I>(input: &mut I, offset: i32) -> winnow::Result<i32>
where
    I: winnow::stream::Stream<Token = char, Slice = &'a str> + winnow::stream::StreamIsPartial,
{
    winnow::ascii::dec_int.parse_next(input).map(|i: i32| i + offset)
}

fn parse_main_inner<'a, I>(input: &mut I) -> winnow::Result<i32>
where
    I: winnow::stream::Stream<Token = char, Slice = &'a str> + winnow::stream::StreamIsPartial,
{
    let mut parser = move |i: &mut _| parse_value_inner(i, 10);
    parser.parse_next(input)
}

fn main() {}

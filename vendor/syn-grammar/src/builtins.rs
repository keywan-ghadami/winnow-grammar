use crate::rt::{self, ParseContext};
use syn::parse::ParseStream;
use syn::Result;

pub fn parse_ident_impl(input: ParseStream, _ctx: &mut ParseContext) -> Result<syn::Ident> {
    rt::parse_ident(input)
}

pub fn parse_integer_impl(input: ParseStream, _ctx: &mut ParseContext) -> Result<i32> {
    rt::parse_int::<i32>(input)
}

pub fn parse_string_impl(input: ParseStream, _ctx: &mut ParseContext) -> Result<String> {
    Ok(input.parse::<syn::LitStr>()?.value())
}

pub fn parse_rust_type_impl(input: ParseStream, _ctx: &mut ParseContext) -> Result<syn::Type> {
    input.parse()
}

pub fn parse_rust_block_impl(input: ParseStream, _ctx: &mut ParseContext) -> Result<syn::Block> {
    input.parse()
}

pub fn parse_lit_str_impl(input: ParseStream, _ctx: &mut ParseContext) -> Result<syn::LitStr> {
    input.parse()
}

pub fn parse_lit_int_impl(input: ParseStream, _ctx: &mut ParseContext) -> Result<syn::LitInt> {
    input.parse()
}

pub fn parse_lit_char_impl(input: ParseStream, _ctx: &mut ParseContext) -> Result<syn::LitChar> {
    input.parse()
}

pub fn parse_lit_bool_impl(input: ParseStream, _ctx: &mut ParseContext) -> Result<syn::LitBool> {
    input.parse()
}

pub fn parse_lit_float_impl(input: ParseStream, _ctx: &mut ParseContext) -> Result<syn::LitFloat> {
    input.parse()
}

pub fn parse_outer_attrs_impl(
    input: ParseStream,
    _ctx: &mut ParseContext,
) -> Result<Vec<syn::Attribute>> {
    syn::Attribute::parse_outer(input)
}

// Spanned variants
pub fn parse_spanned_int_lit_impl(
    input: ParseStream,
    _ctx: &mut ParseContext,
) -> Result<(i32, proc_macro2::Span)> {
    let l = input.parse::<syn::LitInt>()?;
    Ok((l.base10_parse::<i32>()?, l.span()))
}

pub fn parse_spanned_string_lit_impl(
    input: ParseStream,
    _ctx: &mut ParseContext,
) -> Result<(String, proc_macro2::Span)> {
    let l = input.parse::<syn::LitStr>()?;
    Ok((l.value(), l.span()))
}

pub fn parse_spanned_float_lit_impl(
    input: ParseStream,
    _ctx: &mut ParseContext,
) -> Result<(f64, proc_macro2::Span)> {
    let l = input.parse::<syn::LitFloat>()?;
    Ok((l.base10_parse::<f64>()?, l.span()))
}

pub fn parse_spanned_bool_lit_impl(
    input: ParseStream,
    _ctx: &mut ParseContext,
) -> Result<(bool, proc_macro2::Span)> {
    let l = input.parse::<syn::LitBool>()?;
    Ok((l.value, l.span()))
}

pub fn parse_spanned_char_lit_impl(
    input: ParseStream,
    _ctx: &mut ParseContext,
) -> Result<(char, proc_macro2::Span)> {
    let l = input.parse::<syn::LitChar>()?;
    Ok((l.value(), l.span()))
}

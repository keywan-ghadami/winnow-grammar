use crate::rt::ParseContext;
use proc_macro2::Span;
use syn::parse::ParseStream;
use syn::spanned::Spanned;
use syn::Result;
use syn_grammar_model::model::types::{Identifier, SpannedValue, StringLiteral};

// A trait that all token streams must implement so that we can have
// backend-agnostic builtins for common literal types.
pub trait CommonBuiltins {
    fn parse_ident(&mut self) -> Result<Identifier>;
    fn parse_string(&mut self) -> Result<StringLiteral>;

    fn parse_char(&mut self) -> Result<(char, Span)>;
    fn parse_bool(&mut self) -> Result<(bool, Span)>;

    fn parse_i8(&mut self) -> Result<(i8, Span)>;
    fn parse_i16(&mut self) -> Result<(i16, Span)>;
    fn parse_i32(&mut self) -> Result<(i32, Span)>;
    fn parse_i64(&mut self) -> Result<(i64, Span)>;
    fn parse_i128(&mut self) -> Result<(i128, Span)>;
    fn parse_isize(&mut self) -> Result<(isize, Span)>;

    fn parse_u8(&mut self) -> Result<(u8, Span)>;
    fn parse_u16(&mut self) -> Result<(u16, Span)>;
    fn parse_u32(&mut self) -> Result<(u32, Span)>;
    fn parse_u64(&mut self) -> Result<(u64, Span)>;
    fn parse_u128(&mut self) -> Result<(u128, Span)>;
    fn parse_usize(&mut self) -> Result<(usize, Span)>;

    fn parse_f32(&mut self) -> Result<(f32, Span)>;
    fn parse_f64(&mut self) -> Result<(f64, Span)>;

    fn parse_hex_literal(&mut self) -> Result<(u64, Span)>;
    fn parse_oct_literal(&mut self) -> Result<(u64, Span)>;
    fn parse_bin_literal(&mut self) -> Result<(u64, Span)>;
}

impl<'a> CommonBuiltins for ParseStream<'a> {
    fn parse_ident(&mut self) -> Result<Identifier> {
        let t: syn::Ident = self.parse()?;
        Ok(Identifier::new(t.to_string(), t.span()))
    }

    fn parse_string(&mut self) -> Result<StringLiteral> {
        let lit = self.parse::<syn::LitStr>()?;
        Ok(StringLiteral::new(lit.value(), lit.span()))
    }

    fn parse_char(&mut self) -> Result<(char, Span)> {
        let lit = self.parse::<syn::LitChar>()?;
        Ok((lit.value(), lit.span()))
    }

    fn parse_bool(&mut self) -> Result<(bool, Span)> {
        let lit = self.parse::<syn::LitBool>()?;
        Ok((lit.value, lit.span()))
    }

    fn parse_i8(&mut self) -> Result<(i8, Span)> {
        let lit = self.parse::<syn::LitInt>()?;
        Ok((lit.base10_parse()?, lit.span()))
    }

    fn parse_i16(&mut self) -> Result<(i16, Span)> {
        let lit = self.parse::<syn::LitInt>()?;
        Ok((lit.base10_parse()?, lit.span()))
    }

    fn parse_i32(&mut self) -> Result<(i32, Span)> {
        let lit = self.parse::<syn::LitInt>()?;
        Ok((lit.base10_parse()?, lit.span()))
    }

    fn parse_i64(&mut self) -> Result<(i64, Span)> {
        let lit = self.parse::<syn::LitInt>()?;
        Ok((lit.base10_parse()?, lit.span()))
    }

    fn parse_i128(&mut self) -> Result<(i128, Span)> {
        let lit = self.parse::<syn::LitInt>()?;
        Ok((lit.base10_parse()?, lit.span()))
    }

    fn parse_isize(&mut self) -> Result<(isize, Span)> {
        let lit = self.parse::<syn::LitInt>()?;
        Ok((lit.base10_parse()?, lit.span()))
    }

    fn parse_u8(&mut self) -> Result<(u8, Span)> {
        let lit = self.parse::<syn::LitInt>()?;
        Ok((lit.base10_parse()?, lit.span()))
    }

    fn parse_u16(&mut self) -> Result<(u16, Span)> {
        let lit = self.parse::<syn::LitInt>()?;
        Ok((lit.base10_parse()?, lit.span()))
    }

    fn parse_u32(&mut self) -> Result<(u32, Span)> {
        let lit = self.parse::<syn::LitInt>()?;
        Ok((lit.base10_parse()?, lit.span()))
    }

    fn parse_u64(&mut self) -> Result<(u64, Span)> {
        let lit = self.parse::<syn::LitInt>()?;
        Ok((lit.base10_parse()?, lit.span()))
    }

    fn parse_u128(&mut self) -> Result<(u128, Span)> {
        let lit = self.parse::<syn::LitInt>()?;
        Ok((lit.base10_parse()?, lit.span()))
    }

    fn parse_usize(&mut self) -> Result<(usize, Span)> {
        let lit = self.parse::<syn::LitInt>()?;
        Ok((lit.base10_parse()?, lit.span()))
    }

    fn parse_f32(&mut self) -> Result<(f32, Span)> {
        let lit = self.parse::<syn::LitFloat>()?;
        Ok((lit.base10_parse()?, lit.span()))
    }

    fn parse_f64(&mut self) -> Result<(f64, Span)> {
        let lit = self.parse::<syn::LitFloat>()?;
        Ok((lit.base10_parse()?, lit.span()))
    }

    fn parse_hex_literal(&mut self) -> Result<(u64, Span)> {
        let lit = self.parse::<syn::LitInt>()?;
        Ok((lit.base10_parse()?, lit.span()))
    }

    fn parse_oct_literal(&mut self) -> Result<(u64, Span)> {
        let lit = self.parse::<syn::LitInt>()?;
        Ok((lit.base10_parse()?, lit.span()))
    }

    fn parse_bin_literal(&mut self) -> Result<(u64, Span)> {
        let lit = self.parse::<syn::LitInt>()?;
        Ok((lit.base10_parse()?, lit.span()))
    }
}

pub fn parse_ident_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<Identifier> {
    let t = input.parse_ident()?;
    ctx.record_span(t.span);
    Ok(t)
}

pub fn parse_string_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<StringLiteral> {
    let s_lit = input.parse_string()?;
    ctx.record_span(s_lit.span);
    Ok(s_lit)
}

pub fn parse_char_impl<T: CommonBuiltins>(input: &mut T, ctx: &mut ParseContext) -> Result<char> {
    let (val, span) = input.parse_char()?;
    ctx.record_span(span);
    Ok(val)
}

pub fn parse_bool_impl<T: CommonBuiltins>(input: &mut T, ctx: &mut ParseContext) -> Result<bool> {
    let (val, span) = input.parse_bool()?;
    ctx.record_span(span);
    Ok(val)
}

// Spanned Primitives

pub fn parse_spanned_char_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<SpannedValue<char>> {
    let (val, span) = input.parse_char()?;
    ctx.record_span(span);
    Ok(SpannedValue::new(val, span))
}

pub fn parse_spanned_bool_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<SpannedValue<bool>> {
    let (val, span) = input.parse_bool()?;
    ctx.record_span(span);
    Ok(SpannedValue::new(val, span))
}

pub fn parse_spanned_i8_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<SpannedValue<i8>> {
    let (val, span) = input.parse_i8()?;
    ctx.record_span(span);
    Ok(SpannedValue::new(val, span))
}

pub fn parse_spanned_i16_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<SpannedValue<i16>> {
    let (val, span) = input.parse_i16()?;
    ctx.record_span(span);
    Ok(SpannedValue::new(val, span))
}

pub fn parse_spanned_i32_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<SpannedValue<i32>> {
    let (val, span) = input.parse_i32()?;
    ctx.record_span(span);
    Ok(SpannedValue::new(val, span))
}

pub fn parse_spanned_i64_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<SpannedValue<i64>> {
    let (val, span) = input.parse_i64()?;
    ctx.record_span(span);
    Ok(SpannedValue::new(val, span))
}

pub fn parse_spanned_i128_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<SpannedValue<i128>> {
    let (val, span) = input.parse_i128()?;
    ctx.record_span(span);
    Ok(SpannedValue::new(val, span))
}

pub fn parse_spanned_isize_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<SpannedValue<isize>> {
    let (val, span) = input.parse_isize()?;
    ctx.record_span(span);
    Ok(SpannedValue::new(val, span))
}

pub fn parse_spanned_u8_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<SpannedValue<u8>> {
    let (val, span) = input.parse_u8()?;
    ctx.record_span(span);
    Ok(SpannedValue::new(val, span))
}

pub fn parse_spanned_u16_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<SpannedValue<u16>> {
    let (val, span) = input.parse_u16()?;
    ctx.record_span(span);
    Ok(SpannedValue::new(val, span))
}

pub fn parse_spanned_u32_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<SpannedValue<u32>> {
    let (val, span) = input.parse_u32()?;
    ctx.record_span(span);
    Ok(SpannedValue::new(val, span))
}

pub fn parse_spanned_u64_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<SpannedValue<u64>> {
    let (val, span) = input.parse_u64()?;
    ctx.record_span(span);
    Ok(SpannedValue::new(val, span))
}

pub fn parse_spanned_u128_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<SpannedValue<u128>> {
    let (val, span) = input.parse_u128()?;
    ctx.record_span(span);
    Ok(SpannedValue::new(val, span))
}

pub fn parse_spanned_usize_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<SpannedValue<usize>> {
    let (val, span) = input.parse_usize()?;
    ctx.record_span(span);
    Ok(SpannedValue::new(val, span))
}

pub fn parse_spanned_f32_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<SpannedValue<f32>> {
    let (val, span) = input.parse_f32()?;
    ctx.record_span(span);
    Ok(SpannedValue::new(val, span))
}

pub fn parse_spanned_f64_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<SpannedValue<f64>> {
    let (val, span) = input.parse_f64()?;
    ctx.record_span(span);
    Ok(SpannedValue::new(val, span))
}

// Signed Integers
pub fn parse_i8_impl<T: CommonBuiltins>(input: &mut T, ctx: &mut ParseContext) -> Result<i8> {
    let (val, span) = input.parse_i8()?;
    ctx.record_span(span);
    Ok(val)
}

pub fn parse_i16_impl<T: CommonBuiltins>(input: &mut T, ctx: &mut ParseContext) -> Result<i16> {
    let (val, span) = input.parse_i16()?;
    ctx.record_span(span);
    Ok(val)
}

pub fn parse_i32_impl<T: CommonBuiltins>(input: &mut T, ctx: &mut ParseContext) -> Result<i32> {
    let (val, span) = input.parse_i32()?;
    ctx.record_span(span);
    Ok(val)
}

pub fn parse_i64_impl<T: CommonBuiltins>(input: &mut T, ctx: &mut ParseContext) -> Result<i64> {
    let (val, span) = input.parse_i64()?;
    ctx.record_span(span);
    Ok(val)
}

pub fn parse_i128_impl<T: CommonBuiltins>(input: &mut T, ctx: &mut ParseContext) -> Result<i128> {
    let (val, span) = input.parse_i128()?;
    ctx.record_span(span);
    Ok(val)
}

pub fn parse_isize_impl<T: CommonBuiltins>(input: &mut T, ctx: &mut ParseContext) -> Result<isize> {
    let (val, span) = input.parse_isize()?;
    ctx.record_span(span);
    Ok(val)
}

// Unsigned Integers
pub fn parse_u8_impl<T: CommonBuiltins>(input: &mut T, ctx: &mut ParseContext) -> Result<u8> {
    let (val, span) = input.parse_u8()?;
    ctx.record_span(span);
    Ok(val)
}

pub fn parse_u16_impl<T: CommonBuiltins>(input: &mut T, ctx: &mut ParseContext) -> Result<u16> {
    let (val, span) = input.parse_u16()?;
    ctx.record_span(span);
    Ok(val)
}

pub fn parse_u32_impl<T: CommonBuiltins>(input: &mut T, ctx: &mut ParseContext) -> Result<u32> {
    let (val, span) = input.parse_u32()?;
    ctx.record_span(span);
    Ok(val)
}

pub fn parse_u64_impl<T: CommonBuiltins>(input: &mut T, ctx: &mut ParseContext) -> Result<u64> {
    let (val, span) = input.parse_u64()?;
    ctx.record_span(span);
    Ok(val)
}

pub fn parse_u128_impl<T: CommonBuiltins>(input: &mut T, ctx: &mut ParseContext) -> Result<u128> {
    let (val, span) = input.parse_u128()?;
    ctx.record_span(span);
    Ok(val)
}

pub fn parse_usize_impl<T: CommonBuiltins>(input: &mut T, ctx: &mut ParseContext) -> Result<usize> {
    let (val, span) = input.parse_usize()?;
    ctx.record_span(span);
    Ok(val)
}

// Floating Point
pub fn parse_f32_impl<T: CommonBuiltins>(input: &mut T, ctx: &mut ParseContext) -> Result<f32> {
    let (val, span) = input.parse_f32()?;
    ctx.record_span(span);
    Ok(val)
}

pub fn parse_f64_impl<T: CommonBuiltins>(input: &mut T, ctx: &mut ParseContext) -> Result<f64> {
    let (val, span) = input.parse_f64()?;
    ctx.record_span(span);
    Ok(val)
}

// Alternative Bases
pub fn parse_hex_literal_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<u64> {
    let (val, span) = input.parse_hex_literal()?;
    ctx.record_span(span);
    Ok(val)
}

pub fn parse_oct_literal_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<u64> {
    let (val, span) = input.parse_oct_literal()?;
    ctx.record_span(span);
    Ok(val)
}

pub fn parse_bin_literal_impl<T: CommonBuiltins>(
    input: &mut T,
    ctx: &mut ParseContext,
) -> Result<u64> {
    let (val, span) = input.parse_bin_literal()?;
    ctx.record_span(span);
    Ok(val)
}

// Syn Specific Built-ins (Modified to take &mut ParseStream for uniform codegen)

pub fn parse_rust_type_impl(input: &mut ParseStream, ctx: &mut ParseContext) -> Result<syn::Type> {
    let t: syn::Type = (*input).parse()?;
    ctx.record_span(t.span());
    Ok(t)
}

pub fn parse_rust_block_impl(
    input: &mut ParseStream,
    ctx: &mut ParseContext,
) -> Result<syn::Block> {
    let b: syn::Block = (*input).parse()?;
    ctx.record_span(b.span());
    Ok(b)
}

pub fn parse_lit_str_impl(input: &mut ParseStream, ctx: &mut ParseContext) -> Result<syn::LitStr> {
    let t: syn::LitStr = (*input).parse()?;
    ctx.record_span(t.span());
    Ok(t)
}

pub fn parse_lit_int_impl(input: &mut ParseStream, ctx: &mut ParseContext) -> Result<syn::LitInt> {
    let t: syn::LitInt = (*input).parse()?;
    ctx.record_span(t.span());
    Ok(t)
}

pub fn parse_lit_char_impl(
    input: &mut ParseStream,
    ctx: &mut ParseContext,
) -> Result<syn::LitChar> {
    let t: syn::LitChar = (*input).parse()?;
    ctx.record_span(t.span());
    Ok(t)
}

pub fn parse_lit_bool_impl(
    input: &mut ParseStream,
    ctx: &mut ParseContext,
) -> Result<syn::LitBool> {
    let t: syn::LitBool = (*input).parse()?;
    ctx.record_span(t.span());
    Ok(t)
}

pub fn parse_lit_float_impl(
    input: &mut ParseStream,
    ctx: &mut ParseContext,
) -> Result<syn::LitFloat> {
    let t: syn::LitFloat = (*input).parse()?;
    ctx.record_span(t.span());
    Ok(t)
}

pub fn parse_outer_attrs_impl(
    input: &mut ParseStream,
    ctx: &mut ParseContext,
) -> Result<Vec<syn::Attribute>> {
    let attrs = syn::Attribute::parse_outer(input)?;
    if let Some(last) = attrs.last() {
        ctx.record_span(last.span());
    }
    Ok(attrs)
}

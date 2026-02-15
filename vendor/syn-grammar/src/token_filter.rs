//! Token filters for emulating character-level primitives in a token stream.

use syn::parse::ParseStream;
use syn::{Ident, LitInt, Result};

pub fn alpha(input: ParseStream) -> Result<Ident> {
    let ident: Ident = input.parse()?;
    if ident.to_string().chars().all(char::is_alphabetic) {
        Ok(ident)
    } else {
        Err(syn::Error::new(
            ident.span(),
            "expected an alphabetic identifier",
        ))
    }
}

pub fn digit(input: ParseStream) -> Result<LitInt> {
    let lit: LitInt = input.parse()?;
    if lit.base10_digits().chars().all(|c| c.is_ascii_digit()) {
        Ok(lit)
    } else {
        Err(syn::Error::new(lit.span(), "expected a numeric literal"))
    }
}

pub fn alphanumeric(input: ParseStream) -> Result<Ident> {
    let ident: Ident = input.parse()?;
    if ident.to_string().chars().all(char::is_alphanumeric) {
        Ok(ident)
    } else {
        Err(syn::Error::new(
            ident.span(),
            "expected an alphanumeric identifier",
        ))
    }
}

pub fn hex_digit(input: ParseStream) -> Result<LitInt> {
    let lit: LitInt = input.parse()?;
    if lit.base10_digits().chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(lit)
    } else {
        Err(syn::Error::new(lit.span(), "expected a hex literal"))
    }
}

pub fn oct_digit(input: ParseStream) -> Result<LitInt> {
    let lit: LitInt = input.parse()?;
    if lit.base10_digits().chars().all(|c| c.is_digit(8)) {
        Ok(lit)
    } else {
        Err(syn::Error::new(lit.span(), "expected an octal literal"))
    }
}

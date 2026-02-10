#![doc = include_str!("../README.md")]

#[cfg(feature = "syn")]
use proc_macro2::Span;
use std::collections::HashSet;
#[cfg(feature = "syn")]
use syn::ext::IdentExt;
#[cfg(feature = "syn")]
use syn::parse::discouraged::Speculative;
#[cfg(feature = "syn")]
use syn::parse::ParseStream;
#[cfg(feature = "syn")]
use syn::Result;

#[cfg(feature = "testing")]
pub mod testing;

/// Generic symbol table that tracks variable definitions in nested scopes.
#[derive(Clone, Default)]
pub struct ScopeStack {
    scopes: Vec<HashSet<String>>,
}

impl ScopeStack {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashSet::new()],
        }
    }

    pub fn enter_scope(&mut self) {
        self.scopes.push(HashSet::new());
    }

    pub fn exit_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    pub fn define(&mut self, name: impl Into<String>) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.into());
        }
    }

    pub fn is_defined(&self, name: &str) -> bool {
        for scope in self.scopes.iter().rev() {
            if scope.contains(name) {
                return true;
            }
        }
        false
    }

    pub fn scopes(&self) -> &Vec<HashSet<String>> {
        &self.scopes
    }
}

#[cfg(all(feature = "rt", feature = "syn"))]
#[derive(Clone)]
struct ErrorState {
    err: syn::Error,
    is_deep: bool,
}

/// Holds the state for backtracking and error reporting.
/// This must be passed mutably through the parsing chain.
#[cfg(feature = "rt")]
#[derive(Clone)]
pub struct ParseContext {
    is_fatal: bool,
    #[cfg(feature = "syn")]
    best_error: Option<ErrorState>,
    pub scopes: ScopeStack,
    rule_stack: Vec<String>,
}

#[cfg(feature = "rt")]
impl ParseContext {
    pub fn new() -> Self {
        Self {
            is_fatal: false,
            #[cfg(feature = "syn")]
            best_error: None,
            scopes: ScopeStack::new(),
            rule_stack: Vec::new(),
        }
    }

    pub fn set_fatal(&mut self, fatal: bool) {
        self.is_fatal = fatal;
    }

    pub fn check_fatal(&self) -> bool {
        self.is_fatal
    }

    pub fn enter_rule(&mut self, name: &str) {
        self.rule_stack.push(name.to_string());
    }

    pub fn exit_rule(&mut self) {
        self.rule_stack.pop();
    }

    /// Records an error if it is "deeper" than the current best error.
    #[cfg(feature = "syn")]
    pub fn record_error(&mut self, err: syn::Error, start_span: Span) {
        // Heuristic: Compare the error location to the start of the attempt.
        let is_deep = err.span().start() != start_span.start();

        // Enrich error with rule name if available
        let err = if let Some(rule_name) = self.rule_stack.last() {
            let msg = format!("Error in rule '{}': {}", rule_name, err);
            syn::Error::new(err.span(), msg)
        } else {
            err
        };

        match &mut self.best_error {
            None => {
                self.best_error = Some(ErrorState { err, is_deep });
            }
            Some(existing) => {
                // If new is deep and existing is shallow -> Overwrite
                if is_deep && !existing.is_deep {
                    self.best_error = Some(ErrorState { err, is_deep });
                }
            }
        }
    }

    #[cfg(feature = "syn")]
    pub fn take_best_error(&mut self) -> Option<syn::Error> {
        self.best_error.take().map(|s| s.err)
    }

    // --- Symbol Table Methods ---

    pub fn enter_scope(&mut self) {
        self.scopes.enter_scope();
    }

    pub fn exit_scope(&mut self) {
        self.scopes.exit_scope();
    }

    pub fn define(&mut self, name: impl Into<String>) {
        self.scopes.define(name);
    }

    pub fn is_defined(&self, name: &str) -> bool {
        self.scopes.is_defined(name)
    }

    // --- Inspection Methods ---

    pub fn scopes(&self) -> &Vec<HashSet<String>> {
        self.scopes.scopes()
    }

    pub fn rule_stack(&self) -> &Vec<String> {
        &self.rule_stack
    }
}

#[cfg(feature = "rt")]
impl Default for ParseContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Encapsulates a speculative parse attempt.
/// Requires passing the ParseContext to manage error state.
#[cfg(all(feature = "rt", feature = "syn"))]
#[inline]
pub fn attempt<T, F>(input: ParseStream, ctx: &mut ParseContext, parser: F) -> Result<Option<T>>
where
    F: FnOnce(ParseStream, &mut ParseContext) -> Result<T>,
{
    let was_fatal = ctx.check_fatal();
    ctx.set_fatal(false);

    // Snapshot symbol table and rule stack
    let scopes_snapshot = ctx.scopes.clone();
    let rule_stack_snapshot = ctx.rule_stack.clone();

    let start_span = input.span();
    let fork = input.fork();

    // Pass ctx into the closure
    let res = parser(&fork, ctx);

    let is_now_fatal = ctx.check_fatal();

    match res {
        Ok(val) => {
            input.advance_to(&fork);
            ctx.set_fatal(was_fatal);
            Ok(Some(val))
        }
        Err(e) => {
            if is_now_fatal {
                // Restore state
                ctx.scopes = scopes_snapshot;
                ctx.rule_stack = rule_stack_snapshot;

                ctx.set_fatal(true);
                Err(e)
            } else {
                ctx.set_fatal(was_fatal);
                // Record error BEFORE restoring state to capture inner rule context
                ctx.record_error(e, start_span);

                // Restore state
                ctx.scopes = scopes_snapshot;
                ctx.rule_stack = rule_stack_snapshot;

                Ok(None)
            }
        }
    }
}

/// Wrapper around attempt used specifically for recovery blocks.
#[cfg(all(feature = "rt", feature = "syn"))]
#[inline]
pub fn attempt_recover<T, F>(
    input: ParseStream,
    ctx: &mut ParseContext,
    parser: F,
) -> Result<Option<T>>
where
    F: FnOnce(ParseStream, &mut ParseContext) -> Result<T>,
{
    let was_fatal = ctx.check_fatal();
    ctx.set_fatal(false);

    // Snapshot symbol table and rule stack
    let scopes_snapshot = ctx.scopes.clone();
    let rule_stack_snapshot = ctx.rule_stack.clone();

    let start_span = input.span();
    let fork = input.fork();

    let res = parser(&fork, ctx);

    // Always restore fatal state, ignoring whatever happened inside.
    ctx.set_fatal(was_fatal);

    match res {
        Ok(val) => {
            input.advance_to(&fork);
            Ok(Some(val))
        }
        Err(e) => {
            // Record error BEFORE restoring state
            ctx.record_error(e, start_span);

            // Restore state
            ctx.scopes = scopes_snapshot;
            ctx.rule_stack = rule_stack_snapshot;

            Ok(None)
        }
    }
}

// --- Stateless Helpers (No Context Needed) ---

#[cfg(all(feature = "rt", feature = "syn"))]
#[inline]
pub fn parse_ident(input: ParseStream) -> Result<syn::Ident> {
    input.call(syn::Ident::parse_any)
}

#[cfg(all(feature = "rt", feature = "syn"))]
#[inline]
pub fn parse_int<T: std::str::FromStr>(input: ParseStream) -> Result<T>
where
    T::Err: std::fmt::Display,
{
    input.parse::<syn::LitInt>()?.base10_parse()
}

#[cfg(all(feature = "rt", feature = "syn"))]
pub fn skip_until(input: ParseStream, predicate: impl Fn(ParseStream) -> bool) -> Result<()> {
    while !input.is_empty() && !predicate(input) {
        if input.parse::<proc_macro2::TokenTree>().is_err() {
            break;
        }
    }
    Ok(())
}

#[cfg(all(test, feature = "rt", feature = "syn"))]
mod tests {
    use super::*;

    #[test]
    fn test_rule_name_in_error() {
        let mut ctx = ParseContext::new();
        ctx.enter_rule("test_rule");

        let err = syn::Error::new(Span::call_site(), "expected something");
        ctx.record_error(err, Span::call_site());

        let final_err = ctx.take_best_error().unwrap();
        assert_eq!(
            final_err.to_string(),
            "Error in rule 'test_rule': expected something"
        );
    }

    #[test]
    fn test_nested_rule_name_in_error() {
        let mut ctx = ParseContext::new();
        ctx.enter_rule("outer");
        ctx.enter_rule("inner");

        let err = syn::Error::new(Span::call_site(), "fail");
        ctx.record_error(err, Span::call_site());

        let final_err = ctx.take_best_error().unwrap();
        assert_eq!(final_err.to_string(), "Error in rule 'inner': fail");
    }

    #[test]
    fn test_attempt_captures_rule_context() {
        use syn::parse::Parser;

        let mut ctx = ParseContext::new();

        let parser = |input: ParseStream| {
            ctx.enter_rule("outer");

            // We simulate an attempt that fails.
            // attempt returns Result<Option<T>>.
            // If the closure returns Err, attempt records it and returns Ok(None) (if not fatal).
            let _: Option<()> = attempt(input, &mut ctx, |_input, _ctx| {
                Err(syn::Error::new(Span::call_site(), "parse failed"))
            })?;

            ctx.exit_rule();
            Ok(())
        };

        // We parse an empty string. The attempt fails immediately.
        // The outer parser returns Ok(()).
        // But we check ctx.best_error.
        let _ = parser.parse_str("");

        let err = ctx.take_best_error().expect("Error should be recorded");
        assert_eq!(err.to_string(), "Error in rule 'outer': parse failed");
    }
}

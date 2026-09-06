//! Semantic validation for the grammar model.

use crate::model::backend::Backend;
use crate::model::*;
use std::collections::{HashMap, HashSet};
use syn::spanned::Spanned;

pub fn validate<B: Backend>(grammar: &GrammarDefinition) -> syn::Result<()> {
    let builtins = B::get_builtins();
    let builtin_names: HashSet<String> = builtins.iter().map(|b| b.name.to_string()).collect();

    let mut defined_rules = HashSet::new();
    for rule in &grammar.rules {
        if !defined_rules.insert(rule.name.to_string()) {
            return Err(syn::Error::new(
                rule.name.span(),
                format!("Duplicate rule definition: '{}'", rule.name),
            ));
        }
    }

    // Include extern rules as defined
    let mut extern_defs = HashSet::new();
    for er in &grammar.extern_rules {
        extern_defs.insert(er.name.to_string());
    }

    let all_defs: HashSet<_> = grammar
        .rules
        .iter()
        .map(|r| r.name.to_string())
        .chain(extern_defs.iter().cloned())
        .chain(builtin_names.iter().cloned())
        .collect();

    // Only a glob import (`use ...::*;`) can bring unknown rule names into
    // the grammar - in particular inheritance, which is mapped to
    // `use super::Base::*;` (see `model.rs`). A named import brings exactly
    // one known name and must not switch off the check: otherwise every
    // grammar with an ordinary `use` loses the "Undefined rule" message, and
    // a typo in a rule name only shows up as a follow-up error in the
    // generated code.
    let should_validate_rule_calls = !grammar.uses.iter().any(|u| use_tree_has_glob(&u.tree));

    if should_validate_rule_calls {
        for rule in &grammar.rules {
            validate_rule(rule, &all_defs)?;
        }
    }

    validate_argument_counts(grammar)?;

    // Frames: `#[frame]` rules, what they reach, and `par_fold`.
    crate::frame::check(grammar, &builtin_names)?;

    // Perform advanced analysis
    let analysis = crate::analysis::analyze_grammar(grammar);

    // 1. Detect Infinite Recursion (Error)
    for cycle in &analysis.cycles {
        if cycle.len() > 1 {
            let cycle_str = cycle
                .iter()
                .chain(std::iter::once(&cycle[0]))
                .cloned()
                .collect::<Vec<_>>()
                .join(" -> ");

            // A cycle through `WS` is not left recursion by the user but a
            // syntactic rule inside the whitespace: it calls `WS` at its
            // start, and `WS` calls it. That is what the message says - and it
            // points at the rule to change, not at `WS`.
            if let Some(culprit) = cycle.iter().find(|n| *n != "WS") {
                if cycle.iter().any(|n| n == "WS") {
                    let rule = grammar.rules.iter().find(|r| r.name == *culprit).unwrap();
                    let msg = format!(
                        "rule `{name}` is used by `WS` but is syntactic (lowercase): \
                         a syntactic rule skips whitespace at its start by calling `WS`, \
                         so `{cycle}` recurses without consuming input. \
                         Rules used for whitespace must be lexical - name it `{upper}`.",
                        name = culprit,
                        cycle = cycle_str,
                        upper = culprit.to_uppercase(),
                    );
                    return Err(syn::Error::new(rule.name.span(), msg));
                }
            }

            let msg = format!(
                "Indirect left recursion detected (unsupported): {}",
                cycle_str
            );
            let rule_name = &cycle[0];
            let rule = grammar.rules.iter().find(|r| r.name == *rule_name).unwrap();
            return Err(syn::Error::new(rule.name.span(), msg));
        }
    }

    // 2. Warn about Unused Rules
    if should_validate_rule_calls {
        let mut unused: Vec<_> = analysis.unused_rules.iter().collect();
        unused.sort();
        for rule_name in unused {
            // Ignore internal names like _* and special rule "ws" (implicit whitespace)
            if !rule_name.starts_with('_') && rule_name != "ws" {
                eprintln!("warning: Unused rule: '{}'", rule_name);
            }
        }

        // 3. Shadowing / Ambiguity Errors
        if !analysis.errors.is_empty() {
            let mut err = analysis.errors[0].clone();
            for error in analysis.errors.iter().skip(1) {
                err.combine(error.clone());
            }
            return Err(err);
        }
    }

    Ok(())
}

fn validate_rule(rule: &Rule, all_defs: &HashSet<String>) -> syn::Result<()> {
    for variant in &rule.variants {
        validate_pattern_sequence(&variant.pattern, all_defs, &rule.params)?;
    }
    Ok(())
}

fn validate_pattern_sequence(
    patterns: &[ModelPattern],
    all_defs: &HashSet<String>,
    params: &[RuleParameter],
) -> syn::Result<()> {
    for pattern in patterns {
        validate_pattern(pattern, all_defs, params)?;
    }
    Ok(())
}

fn validate_pattern(
    pattern: &ModelPattern,
    all_defs: &HashSet<String>,
    params: &[RuleParameter],
) -> syn::Result<()> {
    match pattern {
        ModelPattern::RuleCall {
            rule_path, args, ..
        } => {
            // Check if it's a multi-segment path (e.g., imported call)
            let rule_name_opt = rule_path.get_ident().map(|ident| ident.to_string());

            if let Some(rule_name_str) = rule_name_opt {
                let rule_name_ident = if let Some(ident) = rule_path.get_ident() {
                    ident
                } else {
                    &rule_path.segments[1].ident
                };

                // Check if rule_name is in all_defs OR in params (as a grammar parameter)
                let is_param = params.iter().any(|p| p.name == *rule_name_ident);

                let is_portable_builtin =
                    rule_name_ident == "separated" || rule_name_ident == "repeated";

                if !all_defs.contains(&rule_name_str) && !is_param && !is_portable_builtin {
                    return Err(syn::Error::new(
                        rule_path.span(),
                        format!("Undefined rule: '{}'", rule_name_str),
                    ));
                }
            } else {
                // Imported/namespaced calls are assumed valid external references.
            }

            for arg in args {
                match arg {
                    Argument::Positional(p) | Argument::Named(_, p) => {
                        validate_pattern(p, all_defs, params)?;
                    }
                }
            }
        }
        ModelPattern::Repeat(inner, _)
        | ModelPattern::Plus(inner, _)
        | ModelPattern::Optional(inner, _)
        | ModelPattern::Bounded { pattern: inner, .. }
        | ModelPattern::SpanBinding(inner, _, _)
        | ModelPattern::Peek(inner, _) => {
            validate_pattern(inner, all_defs, params)?;
        }
        ModelPattern::Count { pattern: inner, .. } => {
            validate_pattern(inner, all_defs, params)?;
        }
        ModelPattern::Fold { pattern: inner, .. } => {
            validate_pattern(inner, all_defs, params)?;
        }
        ModelPattern::Not(inner, _) => {
            validate_pattern(inner, all_defs, params)?;
        }
        ModelPattern::Group { alts, .. } => {
            for (seq, _, _) in alts {
                validate_pattern_sequence(seq, all_defs, params)?;
            }
        }
        ModelPattern::Bracketed(seq, _)
        | ModelPattern::Braced(seq, _)
        | ModelPattern::Parenthesized(seq, _) => {
            validate_pattern_sequence(seq, all_defs, params)?;
        }
        ModelPattern::Recover { body, sync, .. } => {
            validate_pattern(body, all_defs, params)?;
            validate_pattern(sync, all_defs, params)?;
        }
        ModelPattern::Until { pattern, .. } => {
            validate_pattern(pattern, all_defs, params)?;
            validate_no_bindings(pattern)?;
        }
        ModelPattern::LexicalScope(pattern, _) | ModelPattern::SpacedScope(pattern, _) => {
            validate_pattern(pattern, all_defs, params)?;
        }
        ModelPattern::Fail { .. } => {}
        _ => {}
    }
    Ok(())
}

fn validate_no_bindings(pattern: &ModelPattern) -> syn::Result<()> {
    match pattern {
        ModelPattern::Lit { binding, .. } => {
            if binding.is_some() {
                return Err(syn::Error::new(
                    binding.as_ref().unwrap().span(),
                    "Bindings are not allowed inside 'until' patterns.",
                ));
            }
        }
        ModelPattern::RuleCall { binding, args, .. } => {
            if binding.is_some() {
                return Err(syn::Error::new(
                    binding.as_ref().unwrap().span(),
                    "Bindings are not allowed inside 'until' patterns.",
                ));
            }
            for arg in args {
                match arg {
                    Argument::Positional(p) | Argument::Named(_, p) => {
                        validate_no_bindings(p)?;
                    }
                }
            }
        }
        ModelPattern::Group { alts, .. } => {
            for (seq, _, _) in alts {
                for p in seq {
                    validate_no_bindings(p)?;
                }
            }
        }
        ModelPattern::Bracketed(seq, _)
        | ModelPattern::Braced(seq, _)
        | ModelPattern::Parenthesized(seq, _) => {
            for p in seq {
                validate_no_bindings(p)?;
            }
        }
        ModelPattern::Optional(inner, _)
        | ModelPattern::Repeat(inner, _)
        | ModelPattern::Plus(inner, _)
        | ModelPattern::Bounded { pattern: inner, .. }
        | ModelPattern::Peek(inner, _)
        | ModelPattern::Not(inner, _)
        | ModelPattern::Until { pattern: inner, .. } => {
            validate_no_bindings(inner)?;
        }
        ModelPattern::SpanBinding(_, ident, _) => {
            return Err(syn::Error::new(
                ident.span(),
                "Span bindings (@) are not allowed inside 'until' patterns.",
            ));
        }
        ModelPattern::Recover {
            binding,
            body,
            sync,
            ..
        } => {
            if binding.is_some() {
                return Err(syn::Error::new(
                    binding.as_ref().unwrap().span(),
                    "Bindings are not allowed inside 'until' patterns.",
                ));
            }
            validate_no_bindings(body)?;
            validate_no_bindings(sync)?;
        }
        ModelPattern::Count { .. } => {
            // Count hides bindings, so it's safe in until
        }
        ModelPattern::Fold { .. } => {
            // The element's bindings are consumed by `step` and never escape,
            // so a fold is safe inside `until` for the same reason `count` is.
        }
        ModelPattern::LexicalScope(pattern, _) | ModelPattern::SpacedScope(pattern, _) => {
            validate_no_bindings(pattern)?;
        }
        ModelPattern::Cut(_) => {}
        ModelPattern::Fail { .. } => {}
    }
    Ok(())
}

// Argument count validation
fn validate_argument_counts(grammar: &GrammarDefinition) -> syn::Result<()> {
    let rule_map: HashMap<_, _> = grammar
        .rules
        .iter()
        .map(|r| (r.name.to_string(), r))
        .collect();

    for rule in &grammar.rules {
        for variant in &rule.variants {
            // Recursive validation of arguments
            validate_args_recursive(&variant.pattern, &rule_map)?;
        }
    }
    Ok(())
}

fn validate_args_recursive(
    patterns: &[ModelPattern],
    rule_map: &HashMap<String, &Rule>,
) -> syn::Result<()> {
    for pattern in patterns {
        match pattern {
            ModelPattern::RuleCall {
                rule_path, args, ..
            } => {
                let rule_name_opt = rule_path.get_ident().map(|ident| ident.to_string());

                if let Some(rule_name_str) = rule_name_opt {
                    if let Some(target_rule) = rule_map.get(&rule_name_str) {
                        // Removed named argument check to allow named args.

                        if target_rule.params.len() != args.len() {
                            // Check if user likely forgot arguments
                            if args.is_empty() && !target_rule.params.is_empty() {
                                return Err(syn::Error::new(
                                    rule_path.span(),
                                    format!(
                                        "Rule '{}' expects {} argument(s). To pass arguments, use named arguments like '{}(arg=value)'.",
                                        rule_name_str,
                                        target_rule.params.len(),
                                        rule_name_str
                                    ),
                                ));
                            }

                            return Err(syn::Error::new(
                                rule_path.span(),
                                format!(
                                    "Rule '{}' expects {} argument(s), but got {}.",
                                    rule_name_str,
                                    target_rule.params.len(),
                                    args.len()
                                ),
                            ));
                        }
                    }
                }

                // Recursively check arguments (they are patterns)
                for arg in args {
                    match arg {
                        Argument::Positional(p) | Argument::Named(_, p) => {
                            validate_args_recursive(std::slice::from_ref(p), rule_map)?;
                        }
                    }
                }
            }
            ModelPattern::Repeat(inner, _)
            | ModelPattern::Plus(inner, _)
            | ModelPattern::Optional(inner, _)
            | ModelPattern::Bounded { pattern: inner, .. }
            | ModelPattern::SpanBinding(inner, _, _)
            | ModelPattern::Peek(inner, _) => {
                validate_args_recursive(std::slice::from_ref(inner), rule_map)?;
            }
            ModelPattern::Count { pattern: inner, .. } => {
                validate_args_recursive(std::slice::from_ref(inner), rule_map)?;
            }
            ModelPattern::Not(inner, _) => {
                validate_args_recursive(std::slice::from_ref(inner), rule_map)?;
            }
            ModelPattern::Group { alts, .. } => {
                for (seq, _, _) in alts {
                    validate_args_recursive(seq, rule_map)?;
                }
            }
            ModelPattern::Bracketed(seq, _)
            | ModelPattern::Braced(seq, _)
            | ModelPattern::Parenthesized(seq, _) => {
                validate_args_recursive(seq, rule_map)?;
            }
            ModelPattern::Recover { body, sync, .. } => {
                validate_args_recursive(std::slice::from_ref(body), rule_map)?;
                validate_args_recursive(std::slice::from_ref(sync), rule_map)?;
            }
            ModelPattern::Until { pattern, .. } => {
                validate_args_recursive(std::slice::from_ref(pattern), rule_map)?;
            }
            ModelPattern::LexicalScope(pattern, _) | ModelPattern::SpacedScope(pattern, _) => {
                validate_args_recursive(std::slice::from_ref(pattern), rule_map)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Does this `use` tree carry a glob (`::*`) anywhere?
///
/// Recursive, because the glob can sit in a path (`a::b::*`) or in a group
/// (`a::{b, c::*}`).
fn use_tree_has_glob(tree: &syn::UseTree) -> bool {
    match tree {
        syn::UseTree::Glob(_) => true,
        syn::UseTree::Path(p) => use_tree_has_glob(&p.tree),
        syn::UseTree::Group(g) => g.items.iter().any(use_tree_has_glob),
        syn::UseTree::Name(_) | syn::UseTree::Rename(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::backend::BuiltIn;
    use quote::quote;

    struct TestBackend;
    impl Backend for TestBackend {
        fn get_builtins() -> &'static [BuiltIn] {
            &[
                BuiltIn {
                    name: "ident",
                    return_type: "syn::Ident",
                },
                BuiltIn {
                    name: "string",
                    return_type: "String",
                },
            ]
        }
    }

    fn parse_model(input: proc_macro2::TokenStream) -> GrammarDefinition {
        let p_ast: crate::parser::GrammarDefinition = syn::parse2(input).unwrap();
        p_ast.into()
    }

    #[test]
    fn test_undefined_rule() {
        let input = quote! {
            grammar test {
                main = undefined_rule
            }
        };
        let model = parse_model(input);
        let err = validate::<TestBackend>(&model);
        match err {
            Ok(_) => panic!("Expected undefined rule error"),
            Err(e) => assert_eq!(e.to_string(), "Undefined rule: 'undefined_rule'"),
        }
    }

    #[test]
    fn test_duplicate_rule() {
        let input = quote! {
            grammar test {
                main = "a"
                main = "b"
            }
        };
        let model = parse_model(input);
        let err = validate::<TestBackend>(&model).unwrap_err();
        assert_eq!(err.to_string(), "Duplicate rule definition: 'main'");
    }

    #[test]
    fn test_rule_args_mismatch() {
        let input = quote! {
            grammar test {
                main = sub(arg=1)
                sub = "hello"
            }
        };
        let model = parse_model(input);

        let expected_span = model.rules[0].variants[0].pattern[0].span();

        let err = validate::<TestBackend>(&model).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Rule 'sub' expects 0 argument(s), but got 1."
        );
        assert_eq!(format!("{:?}", err.span()), format!("{:?}", expected_span));
    }

    #[test]
    fn test_shadowing_identical() {
        let input = quote! {
            grammar test {
                main
                    = "a"
                    | "a"
            }
        };
        let model = parse_model(input);
        let err = validate::<TestBackend>(&model).unwrap_err();
        assert!(err
            .to_string()
            .contains("Alternative 1 and 2 are identical"));
    }

    #[test]
    fn test_shadowing_prefix() {
        let input = quote! {
            grammar test {
                main
                    = "a"
                    | "a" "b"
            }
        };
        let model = parse_model(input);
        let err = validate::<TestBackend>(&model).unwrap_err();
        assert!(err
            .to_string()
            .contains("Alternative 1 shadows Alternative 2"));
    }

    #[test]
    fn test_no_shadowing() {
        let input = quote! {
            grammar test {
                main
                    = "a" "b"
                    | "a"
            }
        };
        let model = parse_model(input);
        validate::<TestBackend>(&model).unwrap();
    }

    #[test]
    fn test_bug_typed_param() {
        let input = quote! {
            grammar test {
                list<T>(item: Type) = item
            }
        };
        let model = parse_model(input);
        // This fails in 0.7.0 with "Undefined rule: 'item'"
        validate::<TestBackend>(&model).expect("Validation failed for typed parameter");
    }

    #[test]
    fn test_until_binding_fail() {
        let input = quote! {
            grammar test {
                main = until(x: "a")
            }
        };
        let model = parse_model(input);
        let err = validate::<TestBackend>(&model).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Bindings are not allowed inside 'until' patterns."
        );
    }
}

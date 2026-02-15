//! Semantic validation for the grammar model.

use crate::model::*;
use std::collections::{HashMap, HashSet};

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

    let all_defs: HashSet<_> = grammar
        .rules
        .iter()
        .map(|r| r.name.to_string())
        .chain(builtin_names.iter().cloned())
        .collect();

    // If the grammar inherits, we cannot validate rule calls exhaustively,
    // as some rules are defined in the parent. We defer to the Rust compiler.
    let should_validate_rule_calls = grammar.inherits.is_none();

    if should_validate_rule_calls {
        for rule in &grammar.rules {
            validate_rule(rule, &all_defs)?;
        }
    }

    validate_argument_counts(grammar, &builtin_names)?;

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
    params: &[(syn::Ident, syn::Type)],
) -> syn::Result<()> {
    for pattern in patterns {
        validate_pattern(pattern, all_defs, params)?;
    }
    Ok(())
}

fn validate_pattern(
    pattern: &ModelPattern,
    all_defs: &HashSet<String>,
    params: &[(syn::Ident, syn::Type)],
) -> syn::Result<()> {
    match pattern {
        ModelPattern::RuleCall {
            rule_name, args: _, ..
        } => {
            if !all_defs.contains(&rule_name.to_string()) {
                return Err(syn::Error::new(
                    rule_name.span(),
                    format!("Undefined rule: '{}'", rule_name),
                ));
            }
            // Argument count validation is now a separate pass.
        }
        ModelPattern::Repeat(inner, _)
        | ModelPattern::Plus(inner, _)
        | ModelPattern::Optional(inner, _)
        | ModelPattern::SpanBinding(inner, _, _)
        | ModelPattern::Peek(inner, _) => {
            validate_pattern(inner, all_defs, params)?;
        }
        ModelPattern::Not(inner, _) => {
            validate_pattern(inner, all_defs, params)?;
        }
        ModelPattern::Group(variants, _) => {
            for seq in variants {
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
        _ => {}
    }
    Ok(())
}

// Argument count validation
// This is a separate pass because it needs the full rule map.
fn validate_argument_counts(
    grammar: &GrammarDefinition,
    builtin_names: &HashSet<String>,
) -> syn::Result<()> {
    let rule_map: HashMap<_, _> = grammar
        .rules
        .iter()
        .map(|r| (r.name.to_string(), r))
        .collect();

    for rule in &grammar.rules {
        for variant in &rule.variants {
            for pattern in &variant.pattern {
                if let ModelPattern::RuleCall {
                    rule_name, args, ..
                } = pattern
                {
                    let name_str = rule_name.to_string();
                    if let Some(target_rule) = rule_map.get(&name_str) {
                        if target_rule.params.len() != args.len() {
                            return Err(syn::Error::new(
                                rule_name.span(),
                                format!(
                                    "Rule '{}' expects {} argument(s), but got {}.",
                                    rule_name,
                                    target_rule.params.len(),
                                    args.len()
                                ),
                            ));
                        }
                    } else {
                        // It's a built-in or an inherited rule.
                        // We can't validate args for inherited rules here, so we only check built-ins.
                        let is_builtin = builtin_names.contains(&name_str);

                        if is_builtin && !args.is_empty() {
                            return Err(syn::Error::new(
                                rule_name.span(),
                                format!("Built-in rule '{}' does not accept arguments.", rule_name,),
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
                rule main -> () = undefined_rule -> { () }
            }
        };
        let model = parse_model(input);
        let err = validate::<TestBackend>(&model).unwrap_err();
        assert_eq!(err.to_string(), "Undefined rule: 'undefined_rule'");
    }

    #[test]
    fn test_duplicate_rule() {
        let input = quote! {
            grammar test {
                rule main -> () = "a" -> { () }
                rule main -> () = "b" -> { () }
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
                rule main -> () = sub(1) -> { () }
                rule sub -> () = "hello" -> { () }
            }
        };
        let model = parse_model(input);

        // Locate the expected span: rule 'main' -> variant 0 -> pattern 0 ('sub(1)')
        let expected_span = model.rules[0].variants[0].pattern[0].span();

        let err = validate::<TestBackend>(&model).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Rule 'sub' expects 0 argument(s), but got 1."
        );
        assert_eq!(format!("{:?}", err.span()), format!("{:?}", expected_span));
    }

    #[test]
    fn test_builtin_args() {
        let input = quote! {
            grammar test {
                rule main -> () = ident(1) -> { () }
            }
        };
        let model = parse_model(input);

        // Locate the expected span: rule 'main' -> variant 0 -> pattern 0 ('ident(1)')
        let expected_span = model.rules[0].variants[0].pattern[0].span();

        let err = validate::<TestBackend>(&model).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Built-in rule 'ident' does not accept arguments."
        );
        assert_eq!(format!("{:?}", err.span()), format!("{:?}", expected_span));
    }
}

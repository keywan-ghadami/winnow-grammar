// Moved from macros/src/validator.rs
use crate::model::*;
use std::collections::HashMap;
use syn::{Error, Result};

/// Validates the semantic model of the grammar.
///
/// Checks for:
/// - Undefined rules.
/// - Argument count mismatches in rule calls.
/// - Invalid usage of built-in rules.
///
/// Returns `Ok(())` if the grammar is valid, or a `syn::Error` pointing to the location of the issue.
pub fn validate(grammar: &GrammarDefinition, valid_builtins: &[&str]) -> Result<()> {
    let mut defined_rules = HashMap::new();

    for rule in &grammar.rules {
        defined_rules.insert(rule.name.to_string(), rule.params.len());
    }

    for rule in &grammar.rules {
        for variant in &rule.variants {
            validate_patterns(
                &variant.pattern,
                &defined_rules,
                grammar.inherits.is_some(),
                valid_builtins,
            )?;
        }
    }

    Ok(())
}

fn validate_patterns(
    patterns: &[ModelPattern],
    defined_rules: &HashMap<String, usize>,
    has_inheritance: bool,
    valid_builtins: &[&str],
) -> Result<()> {
    for pattern in patterns {
        match pattern {
            ModelPattern::RuleCall {
                rule_name, args, ..
            } => {
                let name_str = rule_name.to_string();

                if valid_builtins.contains(&name_str.as_str()) {
                    if !args.is_empty() {
                        return Err(Error::new(
                            rule_name.span(),
                            format!("Built-in rule '{}' does not accept arguments.", name_str),
                        ));
                    }
                } else if let Some(&param_count) = defined_rules.get(&name_str) {
                    if args.len() != param_count {
                        return Err(Error::new(
                            rule_name.span(),
                            format!(
                                "Rule '{}' expects {} argument(s), but got {}.",
                                name_str,
                                param_count,
                                args.len()
                            ),
                        ));
                    }
                } else if !has_inheritance {
                    return Err(Error::new(
                        rule_name.span(),
                        format!("Undefined rule: '{}'.", name_str),
                    ));
                }
            }
            ModelPattern::Group(alts, _) => {
                for alt in alts {
                    validate_patterns(alt, defined_rules, has_inheritance, valid_builtins)?;
                }
            }
            ModelPattern::Optional(p, _)
            | ModelPattern::Repeat(p, _)
            | ModelPattern::Plus(p, _) => {
                validate_patterns(
                    std::slice::from_ref(p),
                    defined_rules,
                    has_inheritance,
                    valid_builtins,
                )?;
            }
            ModelPattern::Bracketed(p, _)
            | ModelPattern::Braced(p, _)
            | ModelPattern::Parenthesized(p, _) => {
                validate_patterns(p, defined_rules, has_inheritance, valid_builtins)?;
            }
            ModelPattern::Recover { body, sync, .. } => {
                validate_patterns(
                    std::slice::from_ref(body),
                    defined_rules,
                    has_inheritance,
                    valid_builtins,
                )?;
                validate_patterns(
                    std::slice::from_ref(sync),
                    defined_rules,
                    has_inheritance,
                    valid_builtins,
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::GrammarDefinition as ModelGrammar;
    use crate::parser::GrammarDefinition as AstGrammar;
    use quote::quote;

    fn parse_model(input: proc_macro2::TokenStream) -> ModelGrammar {
        let ast: AstGrammar = syn::parse2(input).expect("Failed to parse AST");
        ast.into()
    }

    #[test]
    fn test_undefined_rule() {
        let input = quote! {
            grammar test {
                rule main -> () = undefined_rule -> { () }
            }
        };
        let model = parse_model(input);

        // Locate the expected span: rule 'main' -> variant 0 -> pattern 0 ('undefined_rule')
        let expected_span = model.rules[0].variants[0].pattern[0].span();

        let err = validate(&model, crate::SYN_BUILTINS).unwrap_err();
        assert_eq!(err.to_string(), "Undefined rule: 'undefined_rule'.");

        // Verify that the error span matches the span of the undefined rule usage
        assert_eq!(format!("{:?}", err.span()), format!("{:?}", expected_span));
    }

    #[test]
    fn test_arg_count_mismatch() {
        let input = quote! {
            grammar test {
                rule main -> () = sub(1) -> { () }
                rule sub -> () = "a" -> { () }
            }
        };
        let model = parse_model(input);

        // Locate the expected span: rule 'main' -> variant 0 -> pattern 0 ('sub(1)')
        let expected_span = model.rules[0].variants[0].pattern[0].span();

        let err = validate(&model, crate::SYN_BUILTINS).unwrap_err();
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

        let err = validate(&model, crate::SYN_BUILTINS).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Built-in rule 'ident' does not accept arguments."
        );
        assert_eq!(format!("{:?}", err.span()), format!("{:?}", expected_span));
    }
}

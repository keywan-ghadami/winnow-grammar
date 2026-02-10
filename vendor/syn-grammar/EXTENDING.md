# Building a Custom Parser Backend

This guide is for developers who want to create their own parser generator backend using `syn-grammar` as the frontend (DSL). The primary use case is building a library like `winnow-grammar` that targets a specific parsing library (e.g., `winnow`, `chumsky`) but offers the ergonomic `grammar! { ... }` syntax.

## Why use `syn-grammar` as a frontend?

Writing a parser generator from scratch is hard. You need to:
1.  Design a DSL.
2.  Write a parser for that DSL.
3.  Handle error reporting and spans.
4.  Implement semantic validation (e.g., left recursion checks).

`syn-grammar-model` solves steps 1-4 for you. It parses the standard `grammar! { ... }` block and gives you a clean, validated Abstract Semantic Graph (ASG) ready for code generation.

## Getting Started

Your backend crate (e.g., `winnow-grammar-macros`) should depend on `syn-grammar-model`.

```toml
[dependencies]
syn-grammar-model = "0.2" # Check for latest version
syn = "2.0"
quote = "1.0"
proc-macro2 = "1.0"
```

## The Pipeline

Your macro will typically look like this:

```rust
use proc_macro::TokenStream;
use syn_grammar_model::{parse_grammar_with_builtins, model::GrammarDefinition};

// Define the built-in rules your backend supports.
// These are rules that users can use without defining them (e.g., 'ident', 'digit').
const MY_BACKEND_BUILTINS: &[&str] = &[
    "ident",
    "digit",
    "alpha1",
    // ... add more specific to your backend (e.g. winnow::ascii::digit1)
];

#[proc_macro]
pub fn grammar(input: TokenStream) -> TokenStream {
    // 1. Parse & Validate
    // Use parse_grammar_with_builtins to validate against YOUR set of built-ins.
    // This ensures users get errors if they use 'rust_type' but you don't support it.
    let model = match parse_grammar_with_builtins(input.into(), MY_BACKEND_BUILTINS) {
        Ok(m) => m,
        Err(e) => return e.to_compile_error().into(),
    };

    // 2. Code Generation
    // This is where you write your custom logic to translate the model into Rust code.
    generate_code(model).into()
}

fn generate_code(grammar: GrammarDefinition) -> proc_macro2::TokenStream {
    // ... your codegen implementation
}
```

## The Semantic Model (`syn_grammar_model::model`)

The `GrammarDefinition` struct is your source of truth. It contains:

-   `name`: The name of the grammar module/struct.
-   `rules`: A list of `Rule` definitions.

### `Rule`
-   `name`: The rule name (e.g., `expr`).
-   `return_type`: The Rust return type (e.g., `syn::Expr`).
-   `variants`: A list of alternatives (like `match` arms).

### `RuleVariant`
-   `pattern`: A sequence of `ModelPattern`s that must match.
-   `action`: The Rust code block to execute on success.

### `ModelPattern` (The important part)
This enum represents the structure of the grammar. You need to map each variant to your target library's combinators.

| Pattern | Description | Winnow Equivalent (Concept) |
| :--- | :--- | :--- |
| `Lit(LitStr)` | A string literal (e.g., `"fn"`) | `literal("fn")` |
| `RuleCall { rule_name, .. }` | Calling another rule | `rule_name` |
| `Group(alts)` | `(a | b)` | `alt((a, b))` |
| `Optional(p)` | `p?` | `opt(p)` |
| `Repeat(p)` | `p*` | `repeat(0.., p)` |
| `Plus(p)` | `p+` | `repeat(1.., p)` |
| `Cut` | `=>` | `cut_err` |

## Handling Built-ins

`syn-grammar-model` validates that every rule used is either defined in the grammar or is in the `valid_builtins` list you provided.

When you encounter a `RuleCall` where `rule_name` is one of your built-ins (e.g., `digit`), you should generate code that invokes your backend's implementation for that primitive.

```rust
// In your codegen:
match pattern {
    ModelPattern::RuleCall { rule_name, .. } => {
        if is_builtin(&rule_name) {
            quote! { winnow::ascii::#rule_name } // Map to library function
        } else {
            quote! { #rule_name } // Call generated rule function
        }
    }
    // ...
}
```

## Advanced Analysis

`syn-grammar-model::analysis` provides tools to help you generate better code:

-   **`collect_custom_keywords`**: Finds all string literals that look like keywords. Useful if you need to generate a tokenizer.
-   **`find_cut`**: detect `=>` operators to handle error cutting/commit points.
-   **`split_left_recursive`**: Separates recursive and base cases. This is crucial if your target library doesn't support left recursion natively (most PEGs/combinators don't).

## Example: Winnow-like Codegen Snippet

Here is a simplified example of how you might translate a variant into a `winnow` parser chain.

```rust
fn compile_variant(v: &RuleVariant) -> TokenStream {
    let mut steps = Vec::new();
    
    for pat in &v.pattern {
        let p_code = compile_pattern(pat);
        steps.push(p_code);
    }
    
    // Winnow often uses tuples for sequencing: (p1, p2, p3)
    let sequence = quote! { ( #(#steps),* ) };
    
    // Map the result to the user's action block
    let action = &v.action;
    quote! {
        #sequence.map(|args| #action)
    }
}
```

## Conclusion

By using `syn-grammar-model`, you focus entirely on **how to generate efficient code for your target library**, rather than worrying about parsing grammar syntax or validating rule references.

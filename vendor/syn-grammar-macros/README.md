# syn-grammar-macros

**The code generation engine for `syn-grammar`.**

> **Note:** You should **not** add this crate to your `Cargo.toml` directly. Instead, use the `syn-grammar` crate, which re-exports the macros from this crate.

This crate defines the procedural macros (`grammar!`) that compile the EBNF-like grammar DSL into actual Rust code. While it is an internal implementation detail of `syn-grammar`, understanding its architecture is useful if you intend to write a custom parser backend.

## Responsibilities

1.  **Parsing & Validation**: It delegates parsing, transformation, and semantic validation to `syn-grammar-model`.
2.  **Code Generation**: It transforms the validated model into a Rust module containing `syn`-based parser functions.

## Code Generation Details

The code generation phase (`codegen` module) transforms the semantic model into a Rust module containing `syn` parser functions.

### 1. Rule Generation

For each rule in the grammar, a public function `parse_<rule_name>` and an internal implementation function `parse_<rule_name>_impl` are generated.

-   **`parse_<rule_name>`**: The public entry point. It initializes the `ParseContext` (used for error reporting and state) and calls the implementation. It handles converting internal errors into `syn::Result`.
-   **`parse_<rule_name>_impl`**: The actual parser logic. It takes `input: ParseStream` and `ctx: &mut ParseContext`.

### 2. Pattern Matching

The generator converts EBNF patterns into `syn` parsing calls:

-   **Literals** (`"fn"`): Converted to `input.parse::<Token![fn]>()?` or custom keyword parsing.
-   **Sequences** (`A B`): Generated as a sequence of statements.
-   **Alternatives** (`A | B`):
    -   If the alternatives have unique starting tokens (determined by `peek`), `if input.peek(...)` blocks are generated for efficient dispatch.
    -   Otherwise, `syn::parse::discouraged::Speculative` (via `rt::attempt`) is used to try each alternative in order.
-   **Repetitions** (`A*`): Converted to `while` loops.
-   **Groups** (`(A B)`): Treated as nested sequences.

### 3. Left Recursion Handling

Standard recursive descent parsers loop infinitely on left-recursive rules (e.g., `expr = expr + term`). `syn-grammar` automatically detects direct left recursion and transforms it into an iterative loop:

1.  **Split Variants**: The rule's variants are split into "base cases" (non-recursive) and "recursive cases" (starting with the rule itself).
2.  **Parse Base**: The parser first attempts to match one of the base cases to establish an initial value (`lhs`).
3.  **Loop**: It then enters a `loop`. Inside the loop, it checks if the input matches the "tail" of any recursive variant (the part after the recursive call).
    -   If it matches, the action is executed using the current `lhs` and the parsed tail, updating `lhs` with the result.
    -   If no recursive variant matches, the loop terminates, and `lhs` is returned.

This transformation allows writing natural expression grammars without manual restructuring.

### 4. The Cut Operator (`=>`)

The cut operator is handled during the generation of alternative branches. When a pattern contains `=>`:

1.  The pattern is split into `pre_cut` and `post_cut`.
2.  If `pre_cut` matches successfully, the parser commits to this branch.
3.  Any failure in `post_cut` becomes a fatal error, preventing backtracking to other alternatives.

### 5. Error Reporting

The generated code uses `ParseContext` to track errors. When speculative parsing (`attempt`) fails, the error is recorded. The context keeps track of the "deepest" error (the one that consumed the most tokens) to provide helpful diagnostics to the user, rather than just reporting the last failure.

## Creating a Custom Backend

If you want to generate parsers for a different library (e.g., `winnow`, `chumsky`, or a documentation generator) instead of `syn`, you cannot simply "plug in" a generator to this crate. Procedural macros are compiled as separate artifacts, so the code generation logic must be baked into the macro crate itself.

To create a new backend:

1.  **Create a new proc-macro crate** (e.g., `my-grammar-macros`).
2.  **Depend on `syn-grammar-model`**. This gives you the parser for the DSL, so you don't have to rewrite the grammar syntax parsing.
3.  **Implement your own `codegen` module**. This module will take the `GrammarDefinition` from the model and output your desired code.
4.  **Define your own `grammar!` macro**. This is necessary because of a fundamental limitation in Rust procedural macros: a macro crate must contain the logic it executes. You cannot dynamically inject a generator function into an existing compiled macro crate. Therefore, you must define the macro entry point in your own crate to invoke your custom generator.

Example of a custom backend entry point:

```rust
use proc_macro::TokenStream;
use syn_grammar_model::parse_grammar;
// use my_custom_codegen::generate;

#[proc_macro]
pub fn grammar(input: TokenStream) -> TokenStream {
    // 1. Reuse the shared model parser
    let model = match parse_grammar(input.into()) {
        Ok(m) => m,
        Err(e) => return e.to_compile_error().into(),
    };

    // 2. Use your custom generator
    // let output = generate(model);

    // output.into()
    TokenStream::new() // Placeholder
}
```

This architecture ensures that the syntax remains consistent across different backends while allowing complete flexibility in the generated output.

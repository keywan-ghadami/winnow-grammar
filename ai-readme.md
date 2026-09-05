# winnow-grammar Architecture Notes

## structure
- **Front:** `syn-grammar-model` (AST).
- **Back:** `winnow-grammar` -> `winnow` parsers.
- **Macro:** `winnow-grammar-macros/src/codegen/mod.rs`.

## codegen logic
- **Modes:** 
  - `Syntactic` (`!is_lexical`): Implicit `ws` between steps.
  - `Lexical` (`is_lexical`): Strict, no implicit `ws`.
- **Whitespace (`WS`):**
  - Defaults to `multispace0`. Can be overridden by rule `WS`.


## spans & offsets
- **Wrapper effect:** Wrapper consumes leading `ws`. Inner rule starts *after* `ws`.
- **Spans:** `t:term @ s` -> `s` excludes leading whitespace consumed by wrapper.
- **EOF:** Syntactic `item eof` -> `item` -> `ws` -> `eof`. Input `item <space>` succeeds.

## primitives
- `u32`, `i32` etc map to `winnow::ascii::dec_uint/int`.
- `ident`, `string`, `char` have custom winnow implementations in codegen.

## Important notes for AI agents

1.  **Answer questions, do not act prematurely:** When a yes/no question is asked, give a direct yes or no answer. Do not interpret the question as an implicit request to perform an action.
2.  **Do not invent syntax:** The `grammar!` macro has a very specific, non-generic syntax. Do not try to add generics (e.g. `<'a>`), `where` clauses or other syntax that is not explicitly supported by the macro's definition. The macro is not a standard Rust macro and has its own domain-specific language (DSL).
3.  **Analyse before you act:** Before trying to fix a problem, analyse the existing code, in particular the macros and test patterns, to understand the right approach. Do not make assumptions about how things *should* work.
4.  **Get confirmation:** Before making complex actions or changes to the code, present the plan to the user for approval.
5.  **Use `cargo expand` to understand the macro DSL:** The `grammar!` macro is a domain-specific language (DSL). To understand its correct syntax for more complex use cases (e.g. manual tests without the `test_case!` macro), **do not guess**. Instead, run `cargo expand --test <test_name>` (e.g. `cargo expand --test char_test`) on a working test file. Analyse the expanded code to understand the exact structure the macro generates, and apply that knowledge to write new code.

# Remaining Improvements

## High Priority (Core Features & Correctness)

1. [BLOCKED: syn-grammar support needed] Add support for user-defined custom error types.
2. Optimize usages of the cut operator.
3. [BLOCKED: syn-grammar support needed] Add `expect` combinator for clearer errors (requires pattern wrapper).
4. Implement synchronization points for recovery (refine `recover`).
5. Support user-defined error mapping functions.
6. [BLOCKED: syn-grammar support needed] Support `not` negative lookahead pattern.
7. [BLOCKED: syn-grammar support needed] Allow `where` clauses on grammar rules (for generics).
8. Map `winnow::stream::Location` to spans.
9. Detect infinite recursion loops during compilation.
10. Add diagnostics for grammar conflicts.

## Medium Priority (Optimization & Usability)

11. Optimize `alt` using `dispatch` combinator.
12. Use `preceded` for efficient whitespace skipping.
13. Inline small rules automatically for speed.
14. Support `#[inline]` attribute on rules.
15. Use `dispatch!` for faster keyword matching.
16. Optimize literal matching with `one_of`.
17. Deduplicate common prefixes in alternatives.
18. Generate lookup tables for character classes.
19. Profile generated code to find bottlenecks.
20. Benchmark against handwritten `winnow` parsers.
21. Support `Partial` input streams explicitly.
22. Support `Stateful` input streams in macros.
23. Support custom input types beyond strings.
24. Expose `winnow` combinators directly in grammar.
25. Support `checkpoint` and `reset` manually.
26. Bind to `winnow::token::any` combinator.
27. Support `winnow::token::eof` for end check.
28. Support custom whitespace parsers per rule.
29. Generate `Debug` impls for AST nodes.
30. Allow external rule imports with paths.
31. Support multiple grammar blocks in one file.
32. Generate parsing traces for debugging.
33. Support `#[cfg]` attributes on alternatives.
34. Generate `Display` impl for unparsing.
35. Add property-based testing generator.
36. Improve hygiene of generated rust code.

## Low Priority (Nice to Haves & Extras)

37. Generate `Visitor` trait for AST nodes.
38. Generate `Fold` trait for AST nodes.
39. Implement `fold` or `reduce` actions.
40. Support binary data parsing specific rules.
41. Add bit-level parsing support.
42. Support stateful parsing by passing context.
43. Allow mutable arguments in rules.
44. Support `impl` blocks for grammar state.
45. Add `@` binding for arbitrary expressions.
46. Support regex-like repetition ranges.
47. Create grammar debugger and visualizer tool.
48. Add LSP support for grammar files.
49. Generate railroad diagrams from grammar.
50. Add fuzzer integration for grammars.
51. Improve compile error messages for macros.
52. Add `trace` feature to print steps.
53. Validate left-recursion logic for edge cases.
54. Add examples for parsing JSON format.
55. Add examples for parsing TOML format.
56. Remove unused imports in generated code.
57. Simplify recursive loop generation logic.
58. Refactor `Codegen` struct for readability.
59. Standardize naming of generated functions.
60. Update `syn-grammar` dependency version.
61. Clean up `winnow-grammar-macro` dependencies.
62. Remove `allow(dead_code)` where possible.
63. Unify delimiter handling in codegen.
64. Abstract sequence generation logic further.
65. Improve handling of binding names.
66. Write a tutorial for complex expressions.
67. Document best practices for error recovery.
68. Create a cookbook of common patterns.
69. Add links to `winnow` documentation.
70. Document how to mix handwritten parsers.
71. Explain whitespace handling in depth.
72. Create a migration guide from `nom`.
73. Add badges to README file.
74. Setup CI/CD pipeline for project.
75. Add contributing guidelines for developers.
76. Support semantic actions without AST construction.
77. Allow async parsing rules in grammar.
78. Support streaming parsing mode explicitly.
79. Integrate with `ariadne` for error reports.
80. Support indentation-sensitive grammars.
81. Add macro for testing parsers inline.
82. Support precedence climbing helper utility.
83. Allow defining tokens separately from rules.
84. Support operator overloading syntax in rules.
85. Optimize codegen for `no_std` environments.

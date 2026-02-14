# Remaining Improvements

## High Priority (Core Features & Correctness)

1. Optimize usages of the cut operator.
2. Implement synchronization points for recovery (refine `recover`).
3. Map `winnow::stream::Location` to spans.
4. Detect infinite recursion loops during compilation.
5. Add diagnostics for grammar conflicts.

## Medium Priority (Optimization & Usability)

6. Optimize `alt` using `dispatch` combinator.
7. Use `preceded` for efficient whitespace skipping.
8. Inline small rules automatically for speed.
9. Support `#[inline]` attribute on rules.
10. Use `dispatch!` for faster keyword matching.
11. Optimize literal matching with `one_of`.
12. Deduplicate common prefixes in alternatives.
13. Generate lookup tables for character classes.
14. Profile generated code to find bottlenecks.
15. Benchmark against handwritten `winnow` parsers.
16. Support `Partial` input streams explicitly.
17. Support `Stateful` input streams in macros.
18. Support custom input types beyond strings.
19. Expose `winnow` combinators directly in grammar.
20. Support `checkpoint` and `reset` manually.
21. Bind to `winnow::token::any` combinator.
22. Support `winnow::token::eof` for end check.
23. Support custom whitespace parsers per rule.
24. Generate `Debug` impls for AST nodes.
25. Allow external rule imports with paths.
26. Support multiple grammar blocks in one file.
27. Generate parsing traces for debugging.
28. Support `#[cfg]` attributes on alternatives.
29. Generate `Display` impl for unparsing.
30. Add property-based testing generator.
31. Improve hygiene of generated rust code.

## Low Priority (Nice to Haves & Extras)

32. Generate `Visitor` trait for AST nodes.
33. Generate `Fold` trait for AST nodes.
34. Implement `fold` or `reduce` actions.
35. Support binary data parsing specific rules.
36. Add bit-level parsing support.
37. Support stateful parsing by passing context.
38. Allow mutable arguments in rules.
39. Support `impl` blocks for grammar state.
40. Add `@` binding for arbitrary expressions.
41. Support regex-like repetition ranges.
42. Create grammar debugger and visualizer tool.
43. Add LSP support for grammar files.
44. Generate railroad diagrams from grammar.
45. Add fuzzer integration for grammars.
46. Improve compile error messages for macros.
47. Add `trace` feature to print steps.
48. Validate left-recursion logic for edge cases.
49. Add examples for parsing JSON format.
50. Add examples for parsing TOML format.
51. Remove unused imports in generated code.
52. Simplify recursive loop generation logic.
53. Refactor `Codegen` struct for readability.
54. Standardize naming of generated functions.
55. Update `syn-grammar` dependency version.
56. Clean up `winnow-grammar-macro` dependencies.
57. Remove `allow(dead_code)` where possible.
58. Unify delimiter handling in codegen.
59. Abstract sequence generation logic further.
60. Improve handling of binding names.
61. Write a tutorial for complex expressions.
62. Document best practices for error recovery.
63. Create a cookbook of common patterns.
64. Add links to `winnow` documentation.
65. Document how to mix handwritten parsers.
66. Explain whitespace handling in depth.
67. Create a migration guide from `nom`.
68. Add badges to README file.
69. Setup CI/CD pipeline for project.
70. Add contributing guidelines for developers.
71. Support semantic actions without AST construction.
72. Allow async parsing rules in grammar.
73. Support streaming parsing mode explicitly.
74. Integrate with `ariadne` for error reports.
75. Support indentation-sensitive grammars.
76. Add macro for testing parsers inline.
77. Support precedence climbing helper utility.
78. Allow defining tokens separately from rules.
79. Support operator overloading syntax in rules.
80. Optimize codegen for `no_std` environments.

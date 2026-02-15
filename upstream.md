# Required Upstream Features from syn-grammar

This document outlines key features needed in the `syn-grammar` frontend. Implementing these features is crucial to unblock development and enhance the capabilities of the `winnow-grammar` backend. It is intended to facilitate communication with the `syn-grammar` team about our specific needs.

---

## 1. Parameterized Rules (Generics)

-   **Priority:** High
-   **What we need:** The ability for the `syn-grammar` frontend to parse and model generic, reusable rules, as detailed in `syn-grammar`'s own architecture plan.
-   **Why it's needed:** This is the most significant feature blocking our users from writing cleaner, more maintainable grammars. Without it, users are forced to duplicate logic for common patterns like comma-separated lists, leading to significant boilerplate.
-   **Proposed Solution:** We are fully aligned with the approach detailed in **`syn-grammar` ADR-003: Higher-Order Generic Rules**. The proposed "Macro-Time Monomorphization" strategy is an excellent solution. It correctly preserves trait bounds for `rustc` to verify and, crucially, maintains a static call graph, which ensures compatibility with our existing left-recursion detection. Implementing this ADR would fully address our requirements.
-   **Example of syntax this would enable:**
    ```rust
    // A generic rule for a comma-separated list
    rule separated_list<T>(p: impl Parser<T>) -> Vec<T> =
        first:p rest:("," p)* -> { ... }

    // Using the generic rule
    rule main -> Vec<i32> = separated_list(integer)
    ```

## 2. Analysis of Indirect Left Recursion

-   **Priority:** Medium
-   **What we need:** We need `syn-grammar` to perform the graph-based analysis required to detect indirect (or mutual) left recursion (e.g., `a = b; b = a;`). The resulting analysis should be available to the backend.
-   **Why it's needed:** The `winnow-grammar` backend can already transform *direct* left recursion into efficient iterative parsers. However, to support more complex and natural grammar styles, we need `syn-grammar` to identify the more complex cases of mutual recursion so the backend can generate the correct parsing code.

## 3. Enhanced Diagnostics for Error Reporting

-   **Priority:** Low
-   **What we need:** More specific error types and precise source-span information from `syn-grammar` when it fails to parse a grammar definition.
-   **Why it's needed:** The quality of our compile-time error messages is directly limited by the information `syn-grammar` provides. With better diagnostics, we can give our users much more specific and actionable feedback, pinpointing the exact location and cause of an error in their grammar. This would significantly improve the overall developer experience.

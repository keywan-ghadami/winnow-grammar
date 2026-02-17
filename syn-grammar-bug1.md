# syn-grammar-model Bug: Validator fails for typed parameters

## Issue Description
The `validator` module in `syn-grammar-model` incorrectly flags rule calls as "Undefined rule" if they refer to a grammar parameter that has an explicit type.

## Location
`vendor/syn-grammar-model/src/validator.rs`, function `validate_pattern`.

## The Buggy Code
```rust
let is_param = params
    .iter()
    .any(|(p_name, ty)| p_name == rule_name && ty.is_none());
```
The condition `&& ty.is_none()` restricts valid parameters to only those without explicit types.

## Expected Behavior
The validator should allow any parameter defined in the rule's signature to be used as a rule call, regardless of whether it has an explicit type or not.

## Fix
Remove the `&& ty.is_none()` check.
```rust
let is_param = params
    .iter()
    .any(|(p_name, _)| p_name == rule_name);
```

#[test]
fn test_bug_typed_param() {
    let input = quote::quote! {
        grammar test {
            rule list<T>(item: Type) -> () = item -> { () }
        }
    };
    let model = parse_model(input);
    // This fails in 0.7.0 with "Undefined rule: 'item'"
    validate::<TestBackend>(&model).expect("Validation failed for typed parameter");
}

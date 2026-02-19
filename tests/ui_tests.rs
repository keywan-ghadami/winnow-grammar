#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/literal_bindings.rs");
    t.compile_fail("tests/ui/ambiguity.rs");
    t.compile_fail("tests/ui/recursion.rs");
}

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/advanced_tests.rs");
    t.compile_fail("tests/ui/ambiguity.rs");
    t.compile_fail("tests/ui/recursion.rs");
}

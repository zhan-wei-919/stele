#[test]
fn scene_buffer_is_not_clone() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/compile_fail/scene_buffer_no_clone.rs");
}

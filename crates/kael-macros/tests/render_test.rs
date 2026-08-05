#[test]
fn test_derive_render() {
    use kael_macros::Render;

    #[derive(Render)]
    struct _Element;
}

#[test]
fn action_derive_supports_a_renamed_kael_dependency() {
    use kael_macros::Action;

    #[derive(Clone, PartialEq, Action)]
    #[action(no_json, no_register)]
    struct RenamedAction;

    assert_eq!(
        <RenamedAction as kael_renamed::Action>::name_for_type(),
        "RenamedAction"
    );
}

#[test]
fn cursor_none_needs_no_placeholder_argument() {
    use kael_renamed::{Styled as _, div};

    let _ = div().cursor_none();
}

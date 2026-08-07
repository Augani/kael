#[allow(non_camel_case_types)]
#[derive(Clone, PartialEq, kael_macros::Action)]
#[action(no_json)]
struct r#type;

#[test]
fn test_derive_render() {
    use kael_macros::Render;

    #[derive(Render)]
    struct _Element;
}

#[test]
fn action_derive_normalizes_raw_identifiers() {
    assert_eq!(<r#type as kael_renamed::Action>::name_for_type(), "type");
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
fn action_derive_deserializes_empty_named_structs() {
    use kael_macros::Action;

    #[derive(
        Clone,
        PartialEq,
        Action,
        kael_renamed::private::serde::Deserialize,
        kael_renamed::private::schemars::JsonSchema,
    )]
    #[action(no_register)]
    #[serde(crate = "kael_renamed::private::serde")]
    #[schemars(crate = "kael_renamed::private::schemars")]
    struct EmptyNamed {}

    let action =
        <EmptyNamed as kael_renamed::Action>::build(kael_renamed::private::serde_json::json!({}))
            .unwrap();
    assert_eq!(action.name(), "EmptyNamed");
}

#[test]
fn cursor_none_needs_no_placeholder_argument() {
    use kael_renamed::{Styled as _, div};

    let _ = div().cursor_none();
}

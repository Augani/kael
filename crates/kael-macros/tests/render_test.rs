#[test]
fn test_derive_render() {
    use kael_macros::Render;

    #[derive(Render)]
    struct _Element;
}

#[test]
fn test_derive_context() {
    use kael_macros::{AppContext, VisualContext};
    use kael_renamed::{App, Window};

    #[derive(AppContext, VisualContext)]
    struct _MyCustomContext<'a, 'b> {
        #[app]
        app: &'a mut App,
        #[window]
        window: &'b mut Window,
    }
}

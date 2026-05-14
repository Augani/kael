#[test]
fn test_derive_context() {
    use kael_kael_macros::{AppContext, VisualContext};
    use kael::{App, Window};

    #[derive(AppContext, VisualContext)]
    struct _MyCustomContext<'a, 'b> {
        #[app]
        app: &'a mut App,
        #[window]
        window: &'b mut Window,
    }
}

use kael::*;

fn main() {
    Application::new().run(|cx: &mut App| {
        let workspace = cx.open_workspace();
        workspace.add_panel(Box::new(SidebarPanel::new("sidebar", "Explorer")));
        workspace.add_panel(Box::new(InspectorPanel::new("inspector", "Properties")));
        workspace.add_panel(Box::new(BottomPanel::new("bottom", "Terminal")));

        cx.open_window(WindowOptions::default(), |_window, cx| {
            cx.new(|_cx| EditorView)
        })
        .unwrap();

        cx.activate(true);
    });
}

struct EditorView;

impl Render for EditorView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .child("Workspace Editor")
    }
}

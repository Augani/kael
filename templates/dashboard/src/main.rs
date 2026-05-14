use kael::*;

fn main() {
    Application::new().run(|cx: &mut App| {
        let workspace = cx.open_workspace();
        workspace.add_panel(Box::new(SidebarPanel::new("sidebar", "Navigation")));
        workspace.add_panel(Box::new(BottomPanel::new("bottom", "Logs")));

        cx.open_window(WindowOptions::default(), |_window, cx| {
            cx.new(|_cx| DashboardView)
        }).unwrap();

        cx.activate(true);
    });
}

struct DashboardView;

impl Render for DashboardView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .child("Dashboard")
    }
}

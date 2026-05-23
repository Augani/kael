use kael::*;

fn main() {
    Application::new().run(|cx: &mut App| {
        let workspace = cx.open_workspace();
        workspace.add_panel(Box::new(SidebarPanel::new("sidebar", "Contacts")));
        workspace.add_panel(Box::new(BottomPanel::new("bottom", "Status")));

        cx.open_window(WindowOptions::default(), |_window, cx| {
            cx.new(|_cx| MessageView)
        })
        .unwrap();

        cx.activate(true);
    });
}

struct MessageView;

impl Render for MessageView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().flex().flex_col().size_full().child("Messaging App")
    }
}

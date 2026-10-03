use kael_ui::prelude::*;

actions!(desktop_examples, [Quit]);

pub fn install(cx: &mut App, title: &'static str) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.bind_keys([
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-q", Quit, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-q", Quit, None),
    ]);
    let mut menus = StandardMacMenuBar::new(title)
        .file_menu(file_menu())
        .build();
    let quit = kael::MenuItem::action(format!("Quit {title}"), Quit);
    menus[0].items.push(quit);
    cx.set_menus(menus);
}

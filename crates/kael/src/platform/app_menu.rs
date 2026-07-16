use crate::{Action, App, Platform, SharedString};
use anyhow::Result;
use util::ResultExt;

/// A menu of the application, either a main menu or a submenu
pub struct Menu {
    /// The name of the menu
    pub name: SharedString,

    /// Optional icon data (e.g. PNG/SVG bytes) to display alongside the menu name
    pub icon: Option<Vec<u8>>,

    /// The items in the menu
    pub items: Vec<MenuItem>,
}

impl Menu {
    /// Create an empty application menu.
    pub fn new(name: impl Into<SharedString>) -> Self {
        Self {
            name: name.into(),
            icon: None,
            items: Vec::new(),
        }
    }

    /// Set optional icon bytes for the menu.
    pub fn icon(mut self, icon: impl Into<Vec<u8>>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Append a menu item.
    pub fn item(mut self, item: MenuItem) -> Self {
        self.items.push(item);
        self
    }

    /// Validate this menu and its nested items.
    pub fn validate(&self) -> Result<()> {
        validate_menu(self)
    }

    /// Number of menu items, including nested submenu items.
    pub fn item_count(&self) -> usize {
        count_menu_items(self)
    }

    /// Number of action items, including nested submenu action items.
    pub fn action_count(&self) -> usize {
        count_menu_actions(self)
    }

    /// Whether any item maps to a native OS menu role.
    pub fn has_os_actions(&self) -> bool {
        menu_has_os_action(self)
    }

    /// Whether this menu contains a system-managed submenu.
    pub fn has_system_menus(&self) -> bool {
        menu_has_system_menu(self)
    }

    /// Return a content-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "menu: items {}, actions {}, os actions {}, system menus {}",
            self.item_count(),
            self.action_count(),
            self.has_os_actions(),
            self.has_system_menus()
        )
    }

    /// Create an OwnedMenu from this Menu
    pub fn owned(self) -> OwnedMenu {
        OwnedMenu {
            name: self.name.to_string().into(),
            icon: self.icon,
            items: self.items.into_iter().map(|item| item.owned()).collect(),
        }
    }
}

/// Builder for an application menu.
pub struct MenuBuilder {
    menu: Menu,
}

impl MenuBuilder {
    /// Create an empty menu builder.
    pub fn new(name: impl Into<SharedString>) -> Self {
        Self {
            menu: Menu::new(name),
        }
    }

    /// Create a standard Edit menu with native OS role mappings.
    ///
    /// This mirrors the common browser-runtime `role` menu shape while preserving
    /// Kael's typed action dispatch. The resulting menu contains Undo, Redo,
    /// Cut, Copy, Paste, and Select All with separators between edit groups.
    pub fn standard_edit(
        name: impl Into<SharedString>,
        undo: impl Action,
        redo: impl Action,
        cut: impl Action,
        copy: impl Action,
        paste: impl Action,
        select_all: impl Action,
    ) -> Self {
        Self::new(name)
            .os_action("Undo", undo, OsAction::Undo)
            .os_action("Redo", redo, OsAction::Redo)
            .separator()
            .os_action("Cut", cut, OsAction::Cut)
            .os_action("Copy", copy, OsAction::Copy)
            .os_action("Paste", paste, OsAction::Paste)
            .separator()
            .os_action("Select All", select_all, OsAction::SelectAll)
    }

    /// Set optional icon bytes for the menu.
    pub fn icon(mut self, icon: impl Into<Vec<u8>>) -> Self {
        self.menu.icon = Some(icon.into());
        self
    }

    /// Add an action item.
    pub fn action(mut self, name: impl Into<SharedString>, action: impl Action) -> Self {
        self.menu.items.push(MenuItem::action(name, action));
        self
    }

    /// Add an action item associated with a native OS action.
    pub fn os_action(
        mut self,
        name: impl Into<SharedString>,
        action: impl Action,
        os_action: OsAction,
    ) -> Self {
        self.menu
            .items
            .push(MenuItem::os_action(name, action, os_action));
        self
    }

    /// Add a separator.
    pub fn separator(mut self) -> Self {
        self.menu.items.push(MenuItem::separator());
        self
    }

    /// Add a submenu.
    pub fn submenu(mut self, menu: impl Into<Menu>) -> Self {
        self.menu.items.push(MenuItem::submenu(menu.into()));
        self
    }

    /// Add a system-managed submenu.
    pub fn os_submenu(mut self, name: impl Into<SharedString>, menu_type: SystemMenuType) -> Self {
        self.menu.items.push(MenuItem::os_submenu(name, menu_type));
        self
    }

    /// Return the current menu name.
    pub fn name(&self) -> &SharedString {
        &self.menu.name
    }

    /// Return the current menu items.
    pub fn items(&self) -> &[MenuItem] {
        &self.menu.items
    }

    /// Number of configured menu items, including nested submenu items.
    pub fn item_count(&self) -> usize {
        self.menu.item_count()
    }

    /// Number of configured action items, including nested submenu action items.
    pub fn action_count(&self) -> usize {
        self.menu.action_count()
    }

    /// Whether any configured item maps to a native OS menu role.
    pub fn has_os_actions(&self) -> bool {
        self.menu.has_os_actions()
    }

    /// Whether the menu contains a system-managed submenu.
    pub fn has_system_menus(&self) -> bool {
        self.menu.has_system_menus()
    }

    /// Return a content-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        self.menu.to_text()
    }

    /// Validate the menu before installing it.
    pub fn validate(&self) -> Result<()> {
        self.menu.validate()
    }

    /// Build the validated menu.
    pub fn build_checked(self) -> Result<Menu> {
        self.validate()?;
        Ok(self.menu)
    }

    /// Build the menu.
    pub fn build(self) -> Menu {
        self.menu
    }
}

impl From<MenuBuilder> for Menu {
    fn from(value: MenuBuilder) -> Self {
        value.build()
    }
}

impl From<MenuBuilder> for Vec<Menu> {
    fn from(value: MenuBuilder) -> Self {
        vec![value.build()]
    }
}

/// Builder for an application menu bar.
pub struct MenuBarBuilder {
    menus: Vec<Menu>,
}

/// Checked, inspectable plan for installing an application menu bar.
pub struct MenuBarPlan {
    menus: Vec<Menu>,
}

impl MenuBarPlan {
    /// Top-level menus in install order.
    pub fn menus(&self) -> &[Menu] {
        &self.menus
    }

    /// Consume the plan and return the validated menu bar.
    pub fn into_menus(self) -> Vec<Menu> {
        self.menus
    }

    /// Number of top-level menus.
    pub fn menu_count(&self) -> usize {
        self.menus.len()
    }

    /// Whether this plan contains no top-level menus.
    pub fn is_empty(&self) -> bool {
        self.menus.is_empty()
    }

    /// Top-level menu names in install order.
    pub fn top_level_names(&self) -> Vec<&str> {
        self.menus.iter().map(|menu| menu.name.as_ref()).collect()
    }

    /// Total number of menu items, including nested submenu items.
    pub fn item_count(&self) -> usize {
        self.menus.iter().map(count_menu_items).sum()
    }

    /// Total number of action items, including nested submenu action items.
    pub fn action_count(&self) -> usize {
        self.menus.iter().map(count_menu_actions).sum()
    }

    /// Whether any menu item maps to a native OS menu role.
    pub fn has_os_actions(&self) -> bool {
        self.menus.iter().any(menu_has_os_action)
    }

    /// Whether any menu contains a system-managed submenu.
    pub fn has_system_menus(&self) -> bool {
        self.menus.iter().any(menu_has_system_menu)
    }

    /// Return a content-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "menu bar: {} menus, {} items, {} actions, os actions {}, system menus {}",
            self.menu_count(),
            self.item_count(),
            self.action_count(),
            self.has_os_actions(),
            self.has_system_menus()
        )
    }
}

impl MenuBarBuilder {
    /// Create an empty menu bar builder.
    pub fn new() -> Self {
        Self { menus: Vec::new() }
    }

    /// Add a top-level menu.
    pub fn menu(mut self, menu: impl Into<Menu>) -> Self {
        self.menus.push(menu.into());
        self
    }

    /// Return the configured top-level menus.
    pub fn menus(&self) -> &[Menu] {
        &self.menus
    }

    /// Number of configured top-level menus.
    pub fn menu_count(&self) -> usize {
        self.menus.len()
    }

    /// Whether this builder contains no top-level menus.
    pub fn is_empty(&self) -> bool {
        self.menus.is_empty()
    }

    /// Top-level menu names in configured order.
    pub fn top_level_names(&self) -> Vec<&str> {
        self.menus.iter().map(|menu| menu.name.as_ref()).collect()
    }

    /// Total number of configured menu items, including nested submenu items.
    pub fn item_count(&self) -> usize {
        self.menus.iter().map(count_menu_items).sum()
    }

    /// Total number of configured action items, including nested submenu action items.
    pub fn action_count(&self) -> usize {
        self.menus.iter().map(count_menu_actions).sum()
    }

    /// Whether any configured item maps to a native OS menu role.
    pub fn has_os_actions(&self) -> bool {
        self.menus.iter().any(menu_has_os_action)
    }

    /// Whether any configured menu contains a system-managed submenu.
    pub fn has_system_menus(&self) -> bool {
        self.menus.iter().any(menu_has_system_menu)
    }

    /// Return a content-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "menu bar builder: {} menus, {} items, {} actions, os actions {}, system menus {}",
            self.menu_count(),
            self.item_count(),
            self.action_count(),
            self.has_os_actions(),
            self.has_system_menus()
        )
    }

    /// Validate the menu bar before installing it.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.menus.is_empty(),
            "menu bar must contain at least one menu"
        );

        let mut names = std::collections::HashSet::new();
        for menu in &self.menus {
            menu.validate()?;
            let name = menu.name.as_ref();
            anyhow::ensure!(
                names.insert(name),
                "top-level menu name must be unique: {}",
                name
            );
        }

        Ok(())
    }

    /// Build the validated menu bar.
    pub fn build_checked(self) -> Result<Vec<Menu>> {
        Ok(self.build_plan_checked()?.into_menus())
    }

    /// Build a checked, inspectable menu bar plan.
    pub fn build_plan_checked(self) -> Result<MenuBarPlan> {
        self.validate()?;
        Ok(MenuBarPlan { menus: self.menus })
    }

    /// Build the menu bar.
    pub fn build(self) -> Vec<Menu> {
        self.menus
    }
}

impl Default for MenuBarBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl From<MenuBarBuilder> for Vec<Menu> {
    fn from(value: MenuBarBuilder) -> Self {
        value.build()
    }
}

fn count_menu_items(menu: &Menu) -> usize {
    menu.items
        .iter()
        .map(|item| match item {
            MenuItem::Submenu(submenu) => 1 + count_menu_items(submenu),
            MenuItem::Separator | MenuItem::SystemMenu(_) | MenuItem::Action { .. } => 1,
        })
        .sum()
}

fn count_menu_actions(menu: &Menu) -> usize {
    menu.items
        .iter()
        .map(|item| match item {
            MenuItem::Action { .. } => 1,
            MenuItem::Submenu(submenu) => count_menu_actions(submenu),
            MenuItem::Separator | MenuItem::SystemMenu(_) => 0,
        })
        .sum()
}

fn menu_has_os_action(menu: &Menu) -> bool {
    menu.items.iter().any(|item| match item {
        MenuItem::Action { os_action, .. } => os_action.is_some(),
        MenuItem::Submenu(submenu) => menu_has_os_action(submenu),
        MenuItem::Separator | MenuItem::SystemMenu(_) => false,
    })
}

fn menu_has_system_menu(menu: &Menu) -> bool {
    menu.items.iter().any(|item| match item {
        MenuItem::SystemMenu(_) => true,
        MenuItem::Submenu(submenu) => menu_has_system_menu(submenu),
        MenuItem::Separator | MenuItem::Action { .. } => false,
    })
}

/// Builder for the app icon dock/taskbar context menu.
pub struct DockMenuBuilder {
    items: Vec<MenuItem>,
}

impl DockMenuBuilder {
    /// Create an empty dock menu builder.
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Add an action item.
    pub fn action(mut self, name: impl Into<SharedString>, action: impl Action) -> Self {
        self.items.push(MenuItem::action(name, action));
        self
    }

    /// Add an action item associated with a native OS action.
    pub fn os_action(
        mut self,
        name: impl Into<SharedString>,
        action: impl Action,
        os_action: OsAction,
    ) -> Self {
        self.items
            .push(MenuItem::os_action(name, action, os_action));
        self
    }

    /// Add a separator.
    pub fn separator(mut self) -> Self {
        self.items.push(MenuItem::separator());
        self
    }

    /// Add a submenu.
    pub fn submenu(mut self, menu: impl Into<Menu>) -> Self {
        self.items.push(MenuItem::submenu(menu.into()));
        self
    }

    /// Add an already-constructed menu item.
    pub fn item(mut self, item: MenuItem) -> Self {
        self.items.push(item);
        self
    }

    /// Return the configured dock menu items.
    pub fn items(&self) -> &[MenuItem] {
        &self.items
    }

    /// Validate the dock menu before installing it.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.items.is_empty(),
            "dock menu must contain at least one item"
        );

        let mut has_action = false;
        for item in &self.items {
            validate_menu_item(item)?;
            if contains_action_item(item) {
                has_action = true;
            }
        }

        anyhow::ensure!(
            has_action,
            "dock menu must contain at least one action item"
        );
        Ok(())
    }

    /// Build the validated dock menu.
    pub fn build_checked(self) -> Result<Vec<MenuItem>> {
        self.validate()?;
        Ok(self.items)
    }

    /// Build the dock menu without validation.
    pub fn build(self) -> Vec<MenuItem> {
        self.items
    }
}

impl Default for DockMenuBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl From<DockMenuBuilder> for Vec<MenuItem> {
    fn from(value: DockMenuBuilder) -> Self {
        value.build()
    }
}

fn validate_menu(menu: &Menu) -> Result<()> {
    validate_menu_label(&menu.name, "menu")?;
    anyhow::ensure!(
        !menu.items.is_empty(),
        "menu '{}' must contain at least one item",
        menu.name
    );

    for item in &menu.items {
        validate_menu_item(item)?;
    }

    Ok(())
}

fn validate_menu_item(item: &MenuItem) -> Result<()> {
    match item {
        MenuItem::Separator => {}
        MenuItem::Submenu(submenu) => validate_menu(submenu)?,
        MenuItem::SystemMenu(os_menu) => validate_menu_label(&os_menu.name, "system menu")?,
        MenuItem::Action { name, .. } => validate_menu_label(name, "menu action")?,
    }
    Ok(())
}

fn contains_action_item(item: &MenuItem) -> bool {
    match item {
        MenuItem::Action { .. } => true,
        MenuItem::Submenu(submenu) => submenu.items.iter().any(contains_action_item),
        MenuItem::Separator | MenuItem::SystemMenu(_) => false,
    }
}

fn validate_menu_label(label: &SharedString, kind: &str) -> Result<()> {
    let label = label.as_ref();
    anyhow::ensure!(!label.trim().is_empty(), "{} label cannot be empty", kind);
    anyhow::ensure!(
        label == label.trim(),
        "{} label cannot have leading or trailing whitespace",
        kind
    );
    anyhow::ensure!(
        label.len() <= 128,
        "{} label cannot be longer than 128 bytes",
        kind
    );
    anyhow::ensure!(
        !label.chars().any(|character| character.is_control()),
        "{} label cannot contain control characters",
        kind
    );
    Ok(())
}

/// OS menus are menus that are recognized by the operating system
/// This allows the operating system to provide specialized items for
/// these menus
pub struct OsMenu {
    /// The name of the menu
    pub name: SharedString,

    /// The type of menu
    pub menu_type: SystemMenuType,
}

impl OsMenu {
    /// Create an OwnedOsMenu from this OsMenu
    pub fn owned(self) -> OwnedOsMenu {
        OwnedOsMenu {
            name: self.name.to_string().into(),
            menu_type: self.menu_type,
        }
    }
}

/// The type of system menu
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum SystemMenuType {
    /// The 'Services' menu in the Application menu on macOS
    Services,
}

/// The different kinds of items that can be in a menu
pub enum MenuItem {
    /// A separator between items
    Separator,

    /// A submenu
    Submenu(Menu),

    /// A menu, managed by the system (for example, the Services menu on macOS)
    SystemMenu(OsMenu),

    /// An action that can be performed
    Action {
        /// The name of this menu item
        name: SharedString,

        /// the action to perform when this menu item is selected
        action: Box<dyn Action>,

        /// The OS Action that corresponds to this action, if any
        /// See [`OsAction`] for more information
        os_action: Option<OsAction>,
    },
}

impl MenuItem {
    /// Creates a new menu item that is a separator
    pub fn separator() -> Self {
        Self::Separator
    }

    /// Creates a new menu item that is a submenu
    pub fn submenu(menu: Menu) -> Self {
        Self::Submenu(menu)
    }

    /// Creates a new submenu that is populated by the OS
    pub fn os_submenu(name: impl Into<SharedString>, menu_type: SystemMenuType) -> Self {
        Self::SystemMenu(OsMenu {
            name: name.into(),
            menu_type,
        })
    }

    /// Creates a new menu item that invokes an action
    pub fn action(name: impl Into<SharedString>, action: impl Action) -> Self {
        Self::Action {
            name: name.into(),
            action: Box::new(action),
            os_action: None,
        }
    }

    /// Creates a new menu item that invokes an action and has an OS action
    pub fn os_action(
        name: impl Into<SharedString>,
        action: impl Action,
        os_action: OsAction,
    ) -> Self {
        Self::Action {
            name: name.into(),
            action: Box::new(action),
            os_action: Some(os_action),
        }
    }

    /// Create an OwnedMenuItem from this MenuItem
    pub fn owned(self) -> OwnedMenuItem {
        match self {
            MenuItem::Separator => OwnedMenuItem::Separator,
            MenuItem::Submenu(submenu) => OwnedMenuItem::Submenu(submenu.owned()),
            MenuItem::Action {
                name,
                action,
                os_action,
            } => OwnedMenuItem::Action {
                name: name.into(),
                action,
                os_action,
            },
            MenuItem::SystemMenu(os_menu) => OwnedMenuItem::SystemMenu(os_menu.owned()),
        }
    }
}

/// OS menus are menus that are recognized by the operating system
/// This allows the operating system to provide specialized items for
/// these menus
#[derive(Clone)]
pub struct OwnedOsMenu {
    /// The name of the menu
    pub name: SharedString,

    /// The type of menu
    pub menu_type: SystemMenuType,
}

/// A menu of the application, either a main menu or a submenu
#[derive(Clone)]
pub struct OwnedMenu {
    /// The name of the menu
    pub name: SharedString,

    /// Optional icon data (e.g. PNG/SVG bytes) to display alongside the menu name
    pub icon: Option<Vec<u8>>,

    /// The items in the menu
    pub items: Vec<OwnedMenuItem>,
}

/// The different kinds of items that can be in a menu
pub enum OwnedMenuItem {
    /// A separator between items
    Separator,

    /// A submenu
    Submenu(OwnedMenu),

    /// A menu, managed by the system (for example, the Services menu on macOS)
    SystemMenu(OwnedOsMenu),

    /// An action that can be performed
    Action {
        /// The name of this menu item
        name: String,

        /// the action to perform when this menu item is selected
        action: Box<dyn Action>,

        /// The OS Action that corresponds to this action, if any
        /// See [`OsAction`] for more information
        os_action: Option<OsAction>,
    },
}

impl Clone for OwnedMenuItem {
    fn clone(&self) -> Self {
        match self {
            OwnedMenuItem::Separator => OwnedMenuItem::Separator,
            OwnedMenuItem::Submenu(submenu) => OwnedMenuItem::Submenu(submenu.clone()),
            OwnedMenuItem::Action {
                name,
                action,
                os_action,
            } => OwnedMenuItem::Action {
                name: name.clone(),
                action: action.boxed_clone(),
                os_action: *os_action,
            },
            OwnedMenuItem::SystemMenu(os_menu) => OwnedMenuItem::SystemMenu(os_menu.clone()),
        }
    }
}

// TODO: As part of the global selections refactor, these should
// be moved to GPUI-provided actions that make this association
// without leaking the platform details to GPUI users

/// OS actions are actions that are recognized by the operating system
/// This allows the operating system to provide specialized behavior for
/// these actions
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OsAction {
    /// The 'cut' action
    Cut,

    /// The 'copy' action
    Copy,

    /// The 'paste' action
    Paste,

    /// The 'select all' action
    SelectAll,

    /// The 'undo' action
    Undo,

    /// The 'redo' action
    Redo,
}

pub(crate) fn init_app_menus(platform: &dyn Platform, cx: &App) {
    platform.on_will_open_app_menu(Box::new({
        let cx = cx.to_async();
        move || {
            cx.update(|cx| cx.clear_pending_keystrokes()).ok();
        }
    }));

    platform.on_validate_app_menu_command(Box::new({
        let cx = cx.to_async();
        move |action| {
            cx.update(|cx| cx.is_action_available(action))
                .unwrap_or(false)
        }
    }));

    platform.on_app_menu_action(Box::new({
        let cx = cx.to_async();
        move |action| {
            cx.update(|cx| cx.dispatch_action(action)).log_err();
        }
    }));
}

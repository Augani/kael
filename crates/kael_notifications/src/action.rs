//! Notification actions and categories.

use serde::{Deserialize, Serialize};

/// Additional options for a notification action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ActionOptions {
    /// Whether selecting the action should foreground the application.
    pub foreground: bool,
    /// Whether the action is destructive.
    pub destructive: bool,
    /// Whether authentication should be required before performing the action.
    pub authentication_required: bool,
}

/// A notification action button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationAction {
    /// The unique action identifier.
    pub identifier: String,
    /// The label shown to the user.
    pub title: String,
    /// Additional platform hints for the action.
    pub options: ActionOptions,
    /// An optional placeholder reserved for text-input actions.
    ///
    /// The bundled backends currently reject text-input actions.
    pub text_input_placeholder: Option<String>,
}

/// A category that groups reusable notification actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationCategory {
    /// The category identifier.
    pub identifier: String,
    /// The actions associated with the category.
    pub actions: Vec<NotificationAction>,
}

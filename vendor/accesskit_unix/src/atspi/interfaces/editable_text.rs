// Copyright 2026 Kael contributors.
// Licensed under Apache-2.0 or MIT, matching the upstream adapter.

use accesskit_atspi_common::PlatformNode;
use zbus::{fdo, interface};

pub(crate) struct EditableTextInterface {
    node: PlatformNode,
}

impl EditableTextInterface {
    pub fn new(node: PlatformNode) -> Self {
        Self { node }
    }
    fn map_error(&self) -> impl '_ + FnOnce(accesskit_atspi_common::Error) -> fdo::Error {
        |error| crate::util::map_error_from_node(&self.node, error)
    }
}

#[interface(name = "org.a11y.atspi.EditableText")]
impl EditableTextInterface {
    fn set_text_contents(&self, new_contents: &str) -> fdo::Result<bool> {
        self.node
            .set_text_contents(new_contents)
            .map_err(self.map_error())
    }
    fn insert_text(&self, position: i32, text: &str, length: i32) -> fdo::Result<bool> {
        self.node
            .insert_text(position, text, length)
            .map_err(self.map_error())
    }
    fn delete_text(&self, start_pos: i32, end_pos: i32) -> fdo::Result<bool> {
        self.node
            .delete_text(start_pos, end_pos)
            .map_err(self.map_error())
    }
    fn copy_text(&self, start_pos: i32, end_pos: i32) -> fdo::Result<()> {
        if self
            .node
            .copy_text(start_pos, end_pos)
            .map_err(self.map_error())?
        {
            Ok(())
        } else {
            Err(fdo::Error::NotSupported(
                "native text copy was not accepted".into(),
            ))
        }
    }
    fn cut_text(&self, start_pos: i32, end_pos: i32) -> fdo::Result<bool> {
        self.node
            .cut_text(start_pos, end_pos)
            .map_err(self.map_error())
    }
    fn paste_text(&self, position: i32) -> fdo::Result<bool> {
        self.node.paste_text(position).map_err(self.map_error())
    }
}

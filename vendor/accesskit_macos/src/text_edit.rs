// Copyright 2026 The Kael contributors.
// Licensed under Apache-2.0 or MIT, matching the adapter.

use accesskit::{NodeId, TextSelection, TreeId};

/// One foreground edit or platform clipboard operation.
#[derive(Clone, Debug)]
pub enum TextEditOperation {
    /// Replace the captured range in one edit transaction.
    Replace(String),
    /// Copy the captured range without mutating selection or text.
    Copy,
    /// Copy and remove the captured range in one edit transaction.
    Cut,
    /// Insert platform clipboard text into the captured range.
    Paste,
}

/// A native operation with immutable originating text-run identities.
#[derive(Clone, Debug)]
pub struct TextEditRequest {
    /// Tree containing the owning text control.
    pub target_tree: TreeId,
    /// Owning text control's local identity.
    pub target_node: NodeId,
    /// Original run identities and character boundaries.
    pub selection: TextSelection,
    /// Requested edit or clipboard operation.
    pub operation: TextEditOperation,
}

/// Optional native partial-edit transport, invoked on the main thread.
///
/// Returning `true` means the operation was admitted. Deferred handlers must
/// retain the originating identities and revalidate them before model changes;
/// a selection action followed by an edit action is not an atomic substitute.
pub trait TextEditHandler {
    /// Admit one immutable-origin operation, or return `false` without mutation.
    fn edit_text(&mut self, request: TextEditRequest) -> bool;
}

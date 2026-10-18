// Copyright 2023 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file) or the MIT license (found in
// the LICENSE-MIT file), at your option.

use accesskit::Role;
use accesskit_consumer::{FilterResult, Node, common_filter};

// Collapsed outline rows keep semantic descendants in the consumer model, but
// those descendants must not remain navigable or actionable native elements.
pub(crate) fn filter(node: &Node) -> FilterResult {
    let result = common_filter(node);
    if result == FilterResult::ExcludeSubtree {
        return result;
    }
    let mut parent = node.parent();
    while let Some(ancestor) = parent {
        if ancestor.role() == Role::TreeItem && ancestor.data().is_expanded() == Some(false) {
            return FilterResult::ExcludeSubtree;
        }
        parent = ancestor.parent();
    }
    result
}

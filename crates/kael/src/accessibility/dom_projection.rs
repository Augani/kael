//! Bounded browser semantics with priority for mounted and focused controls.
use super::{AccessibilityId, AccessibilityState, AccessibilityTree};
use std::collections::{HashSet, VecDeque};

pub(crate) fn reachable_nodes(
    tree: &AccessibilityTree,
    limit: usize,
) -> (Vec<AccessibilityId>, bool) {
    if limit == 0 || tree.get(tree.root).is_none() {
        return (Vec::new(), !tree.nodes.is_empty());
    }
    let mut ordered = Vec::with_capacity(tree.node_count().min(limit));
    let mut retained = HashSet::with_capacity(ordered.capacity());
    let mut truncated = false;

    // Frame overlays stay proportional to mounted UI; immutable logical rows
    // and headers are not visited to find the foreground controls. Keep each
    // admitted node's ancestor chain so ARIA ownership cannot become orphaned.
    for focused_only in [true, false] {
        for node in tree.nodes.frame_nodes().filter(|node| {
            if focused_only {
                node.states.contains(AccessibilityState::FOCUSED)
            } else {
                node.bounds.is_some() || node.active_descendant.is_some()
            }
        }) {
            truncated |= !admit_path(tree, node.id, limit, &mut ordered, &mut retained);
            if let Some(active) = node.active_descendant {
                truncated |= !admit_path(tree, active, limit, &mut ordered, &mut retained);
            }
        }
    }

    // Fill the remaining space with a logical sample. The queue and visited
    // set are capped independently, even when one node has millions of children.
    let mut queue = VecDeque::from([tree.root]);
    let mut visited = HashSet::new();
    while let Some(id) = queue.pop_front() {
        if visited.contains(&id) {
            continue;
        }
        if visited.len() == limit {
            truncated = true;
            break;
        }
        visited.insert(id);
        let Some(node) = tree.get(id) else { continue };
        if id != tree.root && node.states.contains(AccessibilityState::HIDDEN) {
            continue;
        }
        if !retained.contains(&id) {
            if ordered.len() == limit {
                truncated = true;
                break;
            }
            retained.insert(id);
            ordered.push(id);
        }
        let available = limit - queue.len();
        truncated |= node.children.len() > available;
        queue.extend(node.children.iter().take(available).copied());
    }
    (ordered, truncated)
}

fn admit_path(
    tree: &AccessibilityTree,
    target: AccessibilityId,
    limit: usize,
    ordered: &mut Vec<AccessibilityId>,
    retained: &mut HashSet<AccessibilityId>,
) -> bool {
    let mut path = Vec::new();
    let mut current = target;
    loop {
        let Some(node) = tree.get(current) else {
            return true;
        };
        if current != tree.root && node.states.contains(AccessibilityState::HIDDEN) {
            return true;
        }
        if retained.contains(&current) {
            break;
        }
        if path.len() == limit || path.contains(&current) {
            return false;
        }
        path.push(current);
        if current == tree.root {
            break;
        }
        let Some(parent) = node.parent else {
            return true;
        };
        current = parent;
    }
    if path.len() > limit - ordered.len() {
        return false;
    }
    for id in path.into_iter().rev() {
        retained.insert(id);
        ordered.push(id);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccessibilityNode, AccessibilityRect, AccessibilityRole};

    #[test]
    fn large_logical_header_branch_does_not_starve_visible_cells_or_focus() {
        let root = AccessibilityNode::new(AccessibilityRole::Window);
        let root_id = root.id;
        let mut tree = AccessibilityTree::new(root);
        let header = AccessibilityNode::new(AccessibilityRole::Row);
        let header_id = header.id;
        tree.insert(header);
        tree.set_parent(header_id, root_id);
        for _ in 0..16_384 {
            let column = AccessibilityNode::new(AccessibilityRole::ColumnHeader);
            let id = column.id;
            tree.insert(column);
            tree.set_parent(id, header_id);
        }
        let row = AccessibilityNode::new(AccessibilityRole::Row);
        let row_id = row.id;
        tree.insert(row);
        tree.set_parent(row_id, root_id);
        let cell = AccessibilityNode::new(AccessibilityRole::Cell)
            .with_bounds(AccessibilityRect::new(0.0, 0.0, 80.0, 28.0));
        let cell_id = cell.id;
        tree.insert(cell);
        tree.set_parent(cell_id, row_id);
        let active = AccessibilityNode::new(AccessibilityRole::Cell);
        let active_id = active.id;
        tree.insert(active);
        tree.set_parent(active_id, row_id);
        tree.nodes.get_mut(&row_id).unwrap().active_descendant = Some(active_id);
        let focused = AccessibilityNode::new(AccessibilityRole::Button)
            .with_states(AccessibilityState::FOCUSED);
        let focused_id = focused.id;
        tree.insert(focused);
        tree.set_parent(focused_id, root_id);
        let (ids, truncated) = reachable_nodes(&tree, 32);
        assert!(truncated);
        assert_eq!(ids.len(), 32);
        for id in [root_id, row_id, cell_id, active_id, focused_id] {
            assert!(ids.contains(&id), "required foreground node was truncated");
        }
        let (focused_ids, _) = reachable_nodes(&tree, 2);
        assert_eq!(focused_ids, vec![root_id, focused_id]);
    }

    #[test]
    fn hidden_ancestor_prunes_mounted_descendants_and_zero_limit_is_empty() {
        let root = AccessibilityNode::new(AccessibilityRole::Window);
        let root_id = root.id;
        let mut tree = AccessibilityTree::new(root);
        let hidden = AccessibilityNode::new(AccessibilityRole::Group)
            .with_states(AccessibilityState::HIDDEN);
        let hidden_id = hidden.id;
        tree.insert(hidden);
        tree.set_parent(hidden_id, root_id);
        let child = AccessibilityNode::new(AccessibilityRole::Button)
            .with_bounds(AccessibilityRect::new(0.0, 0.0, 20.0, 20.0));
        let child_id = child.id;
        tree.insert(child);
        tree.set_parent(child_id, hidden_id);
        assert_eq!(reachable_nodes(&tree, 32), (vec![root_id], false));
        assert_eq!(reachable_nodes(&tree, 0), (vec![], true));
    }
}

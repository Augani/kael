// Feature: platform-parity-browser-runtime-features, Property 8/9: File watcher dispatch and depth limits

use std::path::{Path, PathBuf};

use notify::{
    Event, EventKind,
    event::{CreateKind, ModifyKind, RemoveKind, RenameMode},
};
use proptest::prelude::*;

use crate::{FileWatchEvent, FileWatchOptions, translate_watch_event_for_test};

#[derive(Clone, Debug)]
enum FileOperation {
    Create,
    Modify,
    Delete,
    Rename,
}

fn operation_strategy() -> impl Strategy<Value = FileOperation> {
    prop_oneof![
        Just(FileOperation::Create),
        Just(FileOperation::Modify),
        Just(FileOperation::Delete),
        Just(FileOperation::Rename),
    ]
}

fn watch_root() -> PathBuf {
    PathBuf::from("/tmp/gpui-watch-root")
}

fn path_at_depth(root: &Path, depth: usize, file_name: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    for segment in 1..depth {
        path.push(format!("level-{segment}"));
    }
    path.push(file_name);
    path
}

fn event(kind: EventKind, paths: Vec<PathBuf>) -> Event {
    Event {
        kind,
        paths,
        attrs: Default::default(),
    }
}

fn contains_create(events: &[FileWatchEvent], path: &Path) -> bool {
    events
        .iter()
        .any(|event| matches!(event, FileWatchEvent::Created(found) if found == path))
}

fn contains_modify(events: &[FileWatchEvent], path: &Path) -> bool {
    events
        .iter()
        .any(|event| matches!(event, FileWatchEvent::Modified(found) if found == path))
}

fn contains_delete(events: &[FileWatchEvent], path: &Path) -> bool {
    events
        .iter()
        .any(|event| matches!(event, FileWatchEvent::Deleted(found) if found == path))
}

fn contains_rename(events: &[FileWatchEvent], from: &Path, to: &Path) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            FileWatchEvent::Renamed {
                from: found_from,
                to: found_to
            } if found_from == from && found_to == to
        )
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// **Validates: Requirements 16.1, 16.2**
    ///
    /// For any supported file operation on a watched path, the file watcher
    /// translates the backend event into the expected public callback event and
    /// preserves the affected path.
    #[test]
    fn file_watcher_event_dispatch(operation in operation_strategy()) {
        let root = watch_root();
        let options = FileWatchOptions::recursive();

        let dispatched = match operation {
            FileOperation::Create => {
                let path = root.join("created.txt");
                let events = translate_watch_event_for_test(
                    Ok(event(EventKind::Create(CreateKind::File), vec![path.clone()])),
                    &root,
                    options.clone(),
                );
                contains_create(&events, &path)
            }
            FileOperation::Modify => {
                let path = root.join("modified.txt");
                let events = translate_watch_event_for_test(
                    Ok(event(EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Any)), vec![path.clone()])),
                    &root,
                    options.clone(),
                );
                contains_modify(&events, &path)
            }
            FileOperation::Delete => {
                let path = root.join("deleted.txt");
                let events = translate_watch_event_for_test(
                    Ok(event(EventKind::Remove(RemoveKind::File), vec![path.clone()])),
                    &root,
                    options.clone(),
                );
                contains_delete(&events, &path)
            }
            FileOperation::Rename => {
                let from = root.join("before.txt");
                let to = root.join("after.txt");
                let events = translate_watch_event_for_test(
                    Ok(event(
                        EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                        vec![from.clone(), to.clone()],
                    )),
                    &root,
                    options.clone(),
                );
                contains_rename(&events, &from, &to)
            }
        };

        prop_assert!(dispatched);
    }

    /// **Validates: Requirements 16.3**
    ///
    /// For any configured recursive depth limit `D`, changes at depth `D`
    /// are dispatched while changes deeper than `D` are filtered out.
    #[test]
    fn file_watcher_depth_limiting(max_depth in 1usize..=4) {
        let root = watch_root();
        let options = FileWatchOptions {
            recursive: true,
            max_depth: Some(max_depth),
        };

        let allowed_path = path_at_depth(&root, max_depth, "allowed.txt");
        let blocked_path = path_at_depth(&root, max_depth + 1, "blocked.txt");

        let allowed_events = translate_watch_event_for_test(
            Ok(event(
                EventKind::Create(CreateKind::File),
                vec![allowed_path.clone()],
            )),
            &root,
            options.clone(),
        );
        let blocked_events = translate_watch_event_for_test(
            Ok(event(
                EventKind::Create(CreateKind::File),
                vec![blocked_path.clone()],
            )),
            &root,
            options,
        );

        prop_assert!(contains_create(&allowed_events, &allowed_path));
        prop_assert!(!contains_create(&blocked_events, &blocked_path));
    }
}

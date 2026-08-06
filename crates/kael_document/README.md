# `kael_document`

Document lifecycle primitives for native applications built with Kael or with a custom UI.

The crate owns the repetitive parts of a snapshot-oriented document workflow:

- typed parsing and serialization through your `Document` implementation;
- atomic saves and crash-recovery snapshots;
- bounded undo and redo history;
- content-addressed version history with integrity checks; and
- a bounded, portable recent-document list.

It has no dependency on `kael_ui`. File pickers, editor widgets, menus, file-association declarations, and operating-system recent-item integration remain application concerns.

## Quick start

```no_run
use kael_document::{Document, DocumentController, FileType, Result};

struct Notes;

const NOTE_TYPES: &[FileType] = &[FileType {
    name: "Plain text",
    extensions: &["txt", "md"],
    uti: Some("public.plain-text"),
    mime: Some("text/plain"),
}];

impl Document for Notes {
    type Content = String;

    fn file_types() -> &'static [FileType] {
        NOTE_TYPES
    }

    fn new_untitled() -> Self::Content {
        String::new()
    }

    fn read(data: &[u8], _file_type: &FileType) -> Result<Self::Content> {
        Ok(String::from_utf8(data.to_vec())?)
    }

    fn write(content: &Self::Content, _file_type: &FileType) -> Result<Vec<u8>> {
        Ok(content.as_bytes().to_vec())
    }
}

# fn main() -> Result<()> {
smol::block_on(async {
    let storage = std::env::temp_dir().join("my-app-document-state");
    let controller = DocumentController::<Notes>::new_in("com.example.my-app", storage)?;
    let document = controller.new_document();

    document.modify(|text| text.push_str("Build something useful."))?;
    document.save_as("notes.txt").await?;

    assert!(!document.is_dirty());
    Ok(())
})
# }
```

Use `DocumentController::new` to place Kael's metadata under the platform-standard application data directory. `new_in` is useful when an application already owns a storage root.

## Persistence contract

`modify`, `undo`, `redo`, and version restoration update the in-memory model first, then maintain a recovery snapshot. If recovery persistence fails, the operation returns an error but the in-memory change is retained and remains observable through listeners. A successful primary save is likewise not rolled back when recent-document or version-history bookkeeping fails; the returned error explains that the document itself was saved.

Recovery snapshots for file-backed documents carry the SHA-256 digest of the primary revision they extend. Reopening applies a snapshot only when that baseline still matches, so an external file revision is not silently replaced by stale recovery data. Corrupt, incompatible, and stale snapshots are ignored and removed without blocking the primary document from opening.

Primary saves use a temporary sibling, flush and sync it, then atomically replace the destination. Existing file permissions are preserved. Existing symbolic-link save destinations are resolved to their target so saving does not silently replace the link itself. Recovery and metadata files reject symbolic links and non-regular files; on Unix, Kael creates them with owner-only permissions and secures the platform-standard metadata root.

Save, Save As, revert, and version-restore operations are serialized across clones of one `DocumentHandle`. Recovery updates are serialized with save finalization as well. Concurrent edits are allowed while an asynchronous save is in flight: the saved snapshot becomes the baseline and any newer content stays dirty and recoverable. A stale revert or version restore refuses to overwrite a concurrent edit.

Kael does not lock a document against other processes or a second independently opened handle. Coordinate those writers in the application, or add domain-specific conflict detection when external modification is possible.

## Bounds and performance

- Serialized documents, autosaves, and version blobs are limited to 256 MiB each.
- Controllers retain up to 100 undo snapshots by default, 20 persisted versions per document, and 50 recent documents.
- `with_history_limit` changes the undo bound for documents subsequently created by that controller.
- Undo entries are shared snapshots, but every `modify` call clones the current content model and writes a recovery snapshot synchronously.
- File reads are streamed under their configured bound, and autosave headers are written without duplicating the serialized document buffer.

Keep `Document::read` and `Document::write` deterministic. Batch high-frequency edits, and use an application-specific incremental model for very large editors or media projects. Listener callbacks run synchronously after state is committed; they should return quickly. Keep the returned `Subscription` alive for as long as the listener is needed; dropping it or calling `unsubscribe` unregisters the callback. A listener panic is isolated from the document and from other listeners.

The default recovery location is the operating system's temporary directory. Pass `AutosaveConfig::new(AutosaveLocation::AdjacentToFile)` or a custom location to `with_autosave_config` when recovery data must survive temporary-directory cleanup. Treat version history as local recovery data, not as a collaborative revision system or backup service.

## Platform scope

The portable recent list is JSON metadata owned by the controller and preserves native non-Unicode paths on Unix and Windows. Register file associations in the application bundle or installer, and bridge to the native recent-items API when that integration is appropriate for the product. `platform::support()` reports this boundary; it does not register associations.

API documentation is generated from this README and the public item documentation on [docs.rs](https://docs.rs/kael_document). The broader framework guide lives in the [Kael repository](https://github.com/Augani/kael).

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).

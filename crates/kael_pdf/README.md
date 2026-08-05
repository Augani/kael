# `kael_pdf`

Bounded PDF document primitives for native applications built with Kael or with a custom UI.

The crate provides in-process PDF parsing, metadata and outline discovery, per-page text extraction and search, link discovery, atomic document saves, and a validated sidecar annotation model. It has no dependency on `kael_ui` and no native system-library requirement.

## Quick start

```no_run
use kael_pdf::{PdfDocument, Result};

# fn main() -> Result<()> {
smol::block_on(async {
    let document = PdfDocument::open("manual.pdf").await?;
    println!("{} pages", document.page_count());

    let first_page = document.page(0)?;
    let text = first_page.text()?;
    let matches = first_page.search("installation")?;
    let links = first_page.links()?;

    // A lightweight layout placeholder, not a rendering of PDF graphics.
    let preview = first_page.schematic_preview(1.0).await?;
    assert_eq!(preview.pixels().len(), preview.width() as usize * preview.height() as usize * 4);
    println!("{} text bytes, {} matches, {} links", text.len(), matches.len(), links.len());
    Ok(())
})
# }
```

## Deliberate scope

`schematic_preview` draws extracted-text bars and Kael sidecar annotations into an RGBA placeholder. It does **not** rasterize the PDF content stream, fonts, images, or vector graphics. Use a platform PDF API or a dedicated optional renderer in the application when pixel-faithful pages are required. The naming is intentional so a placeholder can never be mistaken for a rendered legal, financial, or design document.

Kael annotations live in `<document>.annotations.json`; they are not embedded into the PDF and are not visible to other PDF readers. The model supports highlights, notes, free text, ink, and stamps. `has_unsaved_annotations` reports dirty state. Use `save_annotations` to update only the sidecar without rewriting the PDF, or `PdfDocument::save` to write both to a new destination. The sidecar-only path verifies that the PDF has not changed on disk. The crate is not a form editor, digital-signature engine, collaborative review service, or PDF/A validator.

## Persistence and recovery

Document and sidecar writes use a temporary sibling, flush and sync it, then atomically replace the destination. Existing PDF permissions are preserved. Existing symbolic-link destinations are resolved to their target instead of replacing the link. Sidecars reject symbolic links and non-regular files; newly created sidecars use owner-only permissions on Unix.

A stale, malformed, oversized, or unsafe sidecar never prevents a valid PDF from opening. Its annotations are ignored and `annotation_load_warning` describes the problem. If the primary PDF save succeeds but sidecar persistence fails, the returned error states that the PDF itself was saved.

## Resource bounds

- input and saved PDFs: 256 MiB;
- parsed objects: 1,000,000; pages: 100,000;
- extracted text per page: 16 MiB;
- cached text: 64 MiB and 1,024 pages;
- schematic previews: at most 4,096 × 4,096 RGBA pixels;
- cached previews: 128 MiB and 256 entries;
- annotation sidecar: 16 MiB and 100,000 annotations;
- search query: 4 KiB; results per page: 10,000; and
- links per page and outline entries: 10,000 each.

Text extraction, search, links, and annotation mutations are synchronous and may do CPU work while holding the document's internal lock. Page preview generation and document open/save use the blocking worker pool. Keep these operations away from latency-sensitive render or audio callbacks.

PDFs are complex, attacker-controlled containers. These limits constrain Kael-owned allocations and traversal, but the parser still runs in process. Applications accepting hostile documents should use operating-system sandboxing or a dedicated worker process as an additional trust boundary.

Treat every `PdfLinkDestination::Uri` as untrusted input. Kael bounds it and rejects control characters, but the application must allowlist schemes and request user intent before opening an external destination.

API documentation is generated from this README and the public item documentation on [docs.rs](https://docs.rs/kael_pdf). The broader framework guide lives in the [Kael repository](https://github.com/Augani/kael).

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).

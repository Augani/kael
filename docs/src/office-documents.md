# Office and PDF document bytes

Kael's document pipeline keeps file acquisition, persistence, container parsing,
and application semantics separate. That separation lets a suite use the same
Rust code on desktop and the web:

1. `App::prompt_for_files` or `FileUpload` returns bounded `ExternalFile` bytes.
2. `kael_office::OfficePackage::open` parses DOCX, XLSX, or PPTX bytes, while
   `kael_pdf::PdfDocument::from_bytes` parses PDF bytes.
3. The app maps extracted content into its own document, sheet, slide, or canvas
   model and renders that model with ordinary Kael views.
4. `OfficePackage::to_bytes`, `PdfDocument::to_bytes`, or an app serializer
   produces output bytes.
5. `App::save_file_bytes` performs native Save As or a browser Blob download.

There is no browser-only document model and shared business logic does not need
filesystem paths.

## DOCX, XLSX, and PPTX foundation

Enable the `office` feature on `kael`, or depend directly on `kael_office`:

```rust
use kael::office::{OfficePackage, OfficeText};

fn import_office(bytes: &[u8]) -> anyhow::Result<()> {
    let package = OfficePackage::open(bytes)?;
    match package.extract_text()? {
        OfficeText::Document(document) => {
            println!("{} paragraphs", document.paragraphs.len());
        }
        OfficeText::Spreadsheet(workbook) => {
            println!("{} worksheets", workbook.sheets.len());
        }
        OfficeText::Presentation(deck) => {
            println!("{} slides", deck.slides.len());
        }
    }
    Ok(())
}
```

The portable OOXML/OPC API detects standard DOCX/XLSX/PPTX main content types,
lists and reads parts, parses core properties and relationships, resolves safe
internal targets, and safely replaces, adds, or removes raw parts. Export is
deterministic: parts are sorted and ZIP timestamps, permissions, compression,
and compression level are normalized. Unknown parts, including relationships,
are retained byte for byte across a read/export round trip.

Text extraction provides an interchange baseline:

- DOCX: text runs grouped into paragraphs, including table-cell paragraphs,
  tabs, and explicit breaks;
- XLSX: shared strings, inline strings, booleans, numeric values, and cached
  formula results, grouped by worksheet and cell reference; and
- PPTX: DrawingML paragraphs grouped by slide in relationship order.

`kael_office` does not paginate Word files, calculate formulas, lay out slides,
render charts or SmartArt, execute macros or embedded objects, or promise pixel
parity with Microsoft Office. A suite should layer its semantic model, layout
engine, collaboration protocol, and high-fidelity adapters on this bounded
foundation. When adding or removing parts, callers must also maintain content
types and relationships; replacing an existing part is the safest primitive.

## Portable PDF services

Enable `pdf` on `kael`, or use `kael_pdf` directly. `PdfDocument::from_bytes`
and async `open_from_memory` work on desktop and `wasm32-unknown-unknown`. Page
count and size, metadata, outlines, text, search, links, sidecar annotations,
and schematic previews share the same APIs. `annotations_to_bytes` and
`load_annotations_from_bytes` make annotations persistable through
`kael_document`, IndexedDB, or an app service. `to_bytes` provides bounded PDF
bytes for download.

Browsers do not expose arbitrary filesystem paths. `PdfDocument::open`, `save`,
and `save_annotations` return a downcastable `PdfPlatformError` there; use
picker bytes and `save_file_bytes`. Native path methods retain atomic file and
sidecar persistence.

The built-in `schematic_preview` is an extracted-text and annotation
placeholder, not PDF graphics rasterization. It does not draw original fonts,
images, vectors, forms, or signatures. Use a dedicated sandboxed renderer for
pixel-faithful PDF pages.

## Bounds and hostile files

Office input and output are capped at 256 MiB, with at most 65,536 parts, 64 MiB
per expanded part, 256 MiB total expanded data, a 500:1 ratio limit after a
small-file allowance, safe UTF-8 relative names, no duplicates or encryption,
and bounded XML depth, event, text, paragraph, string, and cell work. Traversal,
package escapes, DTDs, unsafe relationship targets, and ambiguous names are
rejected.

PDF input/output is capped at 256 MiB, with bounded object, page, text, cache,
preview, annotation, link, outline, search, and metadata work. See its crate API
for exact per-operation values.

These limits constrain Kael-owned work; they are not a process sandbox. Parse
large or hostile files in a [browser worker](browser-workers.md) or restricted
native worker process so malformed input cannot stall the UI or share the main
application trust boundary.

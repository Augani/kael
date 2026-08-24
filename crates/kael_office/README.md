# `kael_office`

`kael_office` is Kael's portable, byte-only OOXML/OPC foundation for `.docx`,
`.xlsx`, and `.pptx` files. The same crate runs on native targets and
`wasm32-unknown-unknown`: it detects the package type, validates and lists ZIP
parts, reads and safely mutates raw parts, exposes core properties and package
relationships, extracts common document/sheet/slide text, and writes a
deterministic archive.

```rust
use kael_office::{OfficePackage, OfficeText};

# fn inspect(bytes: &[u8]) -> anyhow::Result<()> {
let package = OfficePackage::open(bytes)?;
match package.extract_text()? {
    OfficeText::Document(document) => println!("{} paragraphs", document.paragraphs.len()),
    OfficeText::Spreadsheet(book) => println!("{} sheets", book.sheets.len()),
    OfficeText::Presentation(deck) => println!("{} slides", deck.slides.len()),
}
let deterministic_download = package.to_bytes()?;
# let _ = deterministic_download;
# Ok(())
# }
```

## Security and resource bounds

Input and exported packages are limited to 256 MiB, with at most 65,536 parts,
64 MiB per expanded part, 256 MiB total expanded data, a bounded compression
ratio, validated UTF-8 relative part names, no duplicate names, no encrypted
entries, no path traversal, and bounded XML event/depth/output work. DTDs are
rejected. These checks substantially reduce accidental and hostile resource
amplification, but applications handling adversarial files should still parse
in a worker or sandbox.

## Deliberate compatibility boundary

This crate is an OPC container and extraction layer, not a Word, Excel, or
PowerPoint rendering engine. It does not implement Office layout, formulas,
calculation, charts, macros, tracked changes, embedded-object execution,
pagination, font substitution, or pixel-faithful import. Text extraction covers
Word paragraph text, ordinary/shared/inline spreadsheet cell values, and slide
paragraph text. Raw unknown parts and relationship parts are preserved byte for
byte when the package is rewritten, while ZIP ordering, compression, metadata,
and timestamps are normalized for deterministic output.

Part mutation is intentionally low level. Callers are responsible for keeping
`[Content_Types].xml`, relationship parts, and semantic references consistent
when adding or removing parts. `replace_part` is the safest editing primitive
because it preserves the package graph.

## License

Licensed under the Apache License, Version 2.0.

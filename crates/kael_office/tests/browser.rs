#![cfg(all(target_arch = "wasm32", target_os = "unknown"))]

use std::io::{Cursor, Write as _};

use kael_office::{OfficeFormat, OfficePackage, OfficeText};
use wasm_bindgen_test::wasm_bindgen_test;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn docx_bytes() -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let parts = [
        (
            "[Content_Types].xml",
            br#"<Types><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.as_slice(),
        ),
        (
            "word/document.xml",
            br#"<w:document xmlns:w="w"><w:body><w:p><w:r><w:t>Browser &amp; desktop</w:t></w:r></w:p></w:body></w:document>"#.as_slice(),
        ),
        ("custom/opaque.bin", b"opaque".as_slice()),
    ];
    for (name, bytes) in parts {
        writer.start_file(name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[wasm_bindgen_test]
fn browser_roundtrip_preserves_unknown_parts_and_extracts_word_text() {
    let package = OfficePackage::open(&docx_bytes()).unwrap();
    assert_eq!(package.format(), OfficeFormat::Docx);
    let OfficeText::Document(text) = package.extract_text().unwrap() else {
        panic!("expected document text");
    };
    assert_eq!(text.paragraphs, ["Browser & desktop"]);

    let first = package.to_bytes().unwrap();
    let second = package.to_bytes().unwrap();
    assert_eq!(first, second);
    let reopened = OfficePackage::open(&first).unwrap();
    assert_eq!(
        reopened.read_part("custom/opaque.bin").unwrap(),
        Some(b"opaque".as_slice())
    );
}

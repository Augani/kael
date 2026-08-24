#![cfg(all(target_arch = "wasm32", target_os = "unknown"))]

use kael_pdf::{Annotation, PdfDocument, PdfPlatformError, PdfPoint};
use wasm_bindgen_test::wasm_bindgen_test;

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn pdf_bytes(text: &str) -> Vec<u8> {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)");
    let stream = format!("BT\n/F1 12 Tf\n72 168 Td\n({escaped}) Tj\nET");
    let objects = [
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_string()),
        (
            2,
            "<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_string(),
        ),
        (
            3,
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ),
        (
            4,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 240] /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>".to_string(),
        ),
        (
            5,
            format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
        ),
        (6, "<< /Title (Browser PDF) /Author (Kael) >>".to_string()),
    ];
    let mut pdf = Vec::from(&b"%PDF-1.4\n%\xFF\xFF\xFF\xFF\n"[..]);
    let mut offsets = [0usize; 7];
    for (id, body) in objects {
        offsets[id] = pdf.len();
        pdf.extend_from_slice(format!("{id} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref = pdf.len();
    pdf.extend_from_slice(b"xref\n0 7\n0000000000 65535 f \n");
    for offset in offsets.iter().skip(1) {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size 7 /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref}\n%%EOF\n")
            .as_bytes(),
    );
    pdf
}

#[wasm_bindgen_test]
fn browser_byte_parse_search_annotation_and_export_work() {
    let bytes = pdf_bytes("Portable PDF text");
    let document = PdfDocument::from_bytes(&bytes).unwrap();
    assert_eq!(document.page_count(), 1);
    assert_eq!(document.metadata().title.as_deref(), Some("Browser PDF"));
    let page = document.page(0).unwrap();
    assert!(page.text().unwrap().contains("Portable PDF text"));
    assert_eq!(page.search("pdf").unwrap().len(), 1);
    page.add_annotation(Annotation::Note {
        position: PdfPoint::new(10.0, 20.0),
        text: "browser note".to_string(),
    })
    .unwrap();

    let annotations = document.annotations_to_bytes().unwrap();
    let reopened = PdfDocument::from_bytes(&bytes).unwrap();
    reopened.load_annotations_from_bytes(&annotations).unwrap();
    assert_eq!(reopened.page(0).unwrap().annotations().len(), 1);

    let rewritten = document.to_bytes().unwrap();
    assert_eq!(PdfDocument::from_bytes(&rewritten).unwrap().page_count(), 1);
}

#[wasm_bindgen_test(async)]
async fn browser_path_operations_return_a_typed_error() {
    let error = match PdfDocument::open("browser-has-no-path.pdf").await {
        Ok(_) => panic!("browser path open unexpectedly succeeded"),
        Err(error) => error,
    };
    assert!(error.downcast_ref::<PdfPlatformError>().is_some());
}

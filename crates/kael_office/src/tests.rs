use std::io::{Cursor, Write as _};

use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use super::*;

fn package(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in parts {
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn content_types(main_part: &str, main_type: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="xml" ContentType="application/xml"/>
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Override PartName="/{main_part}" ContentType="{main_type}"/>
  <Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
</Types>"#
    )
}

fn docx_fixture() -> Vec<u8> {
    let types = content_types("word/document.xml", DOCX_MAIN_TYPE);
    package(&[
        (CONTENT_TYPES_PART, types.as_bytes()),
        (
            "_rels/.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="officeDocument" Target="word/document.xml"/><Relationship Id="web" Type="hyperlink" Target="https://example.com" TargetMode="External"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            br#"<w:document xmlns:w="w"><w:body><w:p><w:r><w:t>Hello &amp; welcome</w:t></w:r><w:tab/><w:r><w:t>Kael</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>Table cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p/></w:body></w:document>"#,
        ),
        (
            CORE_PROPERTIES_PART,
            br#"<cp:coreProperties xmlns:cp="cp" xmlns:dc="dc"><dc:title>Suite &amp; Demo</dc:title><dc:creator>Kael</dc:creator><cp:revision>7</cp:revision></cp:coreProperties>"#,
        ),
        ("custom/unknown.bin", b"preserve-me"),
    ])
}

#[test]
fn docx_metadata_text_relationships_and_roundtrip_are_portable() {
    let bytes = docx_fixture();
    let mut office = OfficePackage::open(&bytes).unwrap();
    assert_eq!(office.format(), OfficeFormat::Docx);
    assert_eq!(office.main_part_name(), "word/document.xml");
    assert_eq!(
        office.core_properties().unwrap().title.as_deref(),
        Some("Suite & Demo")
    );
    assert_eq!(
        office.core_properties().unwrap().revision.as_deref(),
        Some("7")
    );

    let OfficeText::Document(text) = office.extract_text().unwrap() else {
        panic!("expected Word text");
    };
    assert_eq!(text.paragraphs, ["Hello & welcome\tKael", "Table cell", ""]);

    let relationships = office.relationships(None).unwrap();
    assert_eq!(
        office
            .resolve_relationship_target(None, &relationships[0])
            .unwrap()
            .as_deref(),
        Some("word/document.xml")
    );
    assert_eq!(
        office
            .resolve_relationship_target(None, &relationships[1])
            .unwrap(),
        None
    );

    office
        .replace_part(
            "word/document.xml",
            br#"<w:document xmlns:w="w"><w:body><w:p><w:r><w:t>Edited</w:t></w:r></w:p></w:body></w:document>"#.to_vec(),
        )
        .unwrap();
    office.add_part("custom/new.bin", b"new".to_vec()).unwrap();
    assert_eq!(office.remove_part("custom/new.bin").unwrap(), b"new");
    let first = office.to_bytes().unwrap();
    let second = office.to_bytes().unwrap();
    assert_eq!(first, second);

    let reopened = OfficePackage::open(&first).unwrap();
    assert_eq!(
        reopened.read_part("/custom/unknown.bin").unwrap(),
        Some(b"preserve-me".as_slice())
    );
    let OfficeText::Document(text) = reopened.extract_text().unwrap() else {
        panic!("expected Word text");
    };
    assert_eq!(text.paragraphs, ["Edited"]);
}

#[test]
fn extracts_shared_inline_boolean_and_numeric_spreadsheet_cells() {
    let types = content_types("xl/workbook.xml", XLSX_MAIN_TYPE);
    let bytes = package(&[
        (CONTENT_TYPES_PART, types.as_bytes()),
        (
            "xl/workbook.xml",
            br#"<workbook xmlns:r="r"><sheets><sheet name="Budget" sheetId="1" r:id="rId9"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            br#"<Relationships><Relationship Id="rId9" Type="worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#,
        ),
        (
            "xl/sharedStrings.xml",
            br#"<sst><si><t>Revenue</t></si><si><r><t>Rich </t></r><r><t>text</t></r></si></sst>"#,
        ),
        (
            "xl/worksheets/sheet2.xml",
            br#"<worksheet><sheetData><row><c r="A1" t="s"><v>0</v></c><c r="B1"><v>42.5</v></c><c r="C1" t="b"><v>1</v></c><c r="D1" t="inlineStr"><is><t>Inline</t></is></c><c r="E1" t="s"><v>1</v></c></row></sheetData></worksheet>"#,
        ),
    ]);
    let office = OfficePackage::open(&bytes).unwrap();
    let OfficeText::Spreadsheet(workbook) = office.extract_text().unwrap() else {
        panic!("expected workbook text");
    };
    assert_eq!(workbook.sheets[0].name, "Budget");
    assert_eq!(
        workbook.sheets[0]
            .cells
            .iter()
            .map(|cell| (cell.reference.as_str(), cell.value.as_str()))
            .collect::<Vec<_>>(),
        [
            ("A1", "Revenue"),
            ("B1", "42.5"),
            ("C1", "true"),
            ("D1", "Inline"),
            ("E1", "Rich text"),
        ]
    );
}

#[test]
fn extracts_slides_in_presentation_relationship_order() {
    let types = content_types("ppt/presentation.xml", PPTX_MAIN_TYPE);
    let bytes = package(&[
        (CONTENT_TYPES_PART, types.as_bytes()),
        (
            "ppt/presentation.xml",
            br#"<p:presentation xmlns:p="p" xmlns:r="r"><p:sldIdLst><p:sldId id="5" r:id="second"/><p:sldId id="4" r:id="first"/></p:sldIdLst></p:presentation>"#,
        ),
        (
            "ppt/_rels/presentation.xml.rels",
            br#"<Relationships><Relationship Id="first" Type="slide" Target="slides/slide1.xml"/><Relationship Id="second" Type="slide" Target="slides/slide2.xml"/></Relationships>"#,
        ),
        (
            "ppt/slides/slide1.xml",
            br#"<p:sld xmlns:p="p" xmlns:a="a"><a:p><a:r><a:t>First</a:t></a:r></a:p></p:sld>"#,
        ),
        (
            "ppt/slides/slide2.xml",
            br#"<p:sld xmlns:p="p" xmlns:a="a"><a:p><a:r><a:t>Second</a:t></a:r></a:p></p:sld>"#,
        ),
    ]);
    let office = OfficePackage::open(&bytes).unwrap();
    let OfficeText::Presentation(deck) = office.extract_text().unwrap() else {
        panic!("expected presentation text");
    };
    assert_eq!(deck.slides[0].paragraphs, ["Second"]);
    assert_eq!(deck.slides[1].paragraphs, ["First"]);
}

#[test]
fn rejects_traversal_duplicates_dtd_and_excessive_compression() {
    let types = content_types("word/document.xml", DOCX_MAIN_TYPE);
    let traversal = package(&[
        (CONTENT_TYPES_PART, types.as_bytes()),
        ("word/document.xml", b"<document/>"),
        ("../outside", b"bad"),
    ]);
    assert!(
        OfficePackage::open(&traversal)
            .unwrap_err()
            .to_string()
            .contains("unsafe")
    );

    let dtd = package(&[
        (CONTENT_TYPES_PART, types.as_bytes()),
        (
            "word/document.xml",
            b"<!DOCTYPE x><w:document xmlns:w=\"w\"/>",
        ),
    ]);
    let dtd = OfficePackage::open(&dtd).unwrap();
    assert!(dtd.extract_text().unwrap_err().to_string().contains("DTD"));

    let highly_compressible = vec![b'x'; 2 * 1024 * 1024];
    let bomb = package(&[
        (CONTENT_TYPES_PART, types.as_bytes()),
        ("word/document.xml", b"<document/>"),
        ("custom/bomb.bin", &highly_compressible),
    ]);
    assert!(
        OfficePackage::open(&bomb)
            .unwrap_err()
            .to_string()
            .contains("compression-ratio")
    );
}

#[test]
fn main_manifest_and_oversized_part_mutations_are_guarded() {
    let mut office = OfficePackage::open(&docx_fixture()).unwrap();
    assert!(office.remove_part(CONTENT_TYPES_PART).is_err());
    assert!(office.remove_part("word/document.xml").is_err());
    assert!(office.add_part("../bad", Vec::new()).is_err());
    assert!(
        office
            .add_part("too-large", vec![0; MAX_PART_BYTES as usize + 1])
            .is_err()
    );

    let xlsx_types = content_types("xl/workbook.xml", XLSX_MAIN_TYPE);
    assert!(
        office
            .replace_part(CONTENT_TYPES_PART, xlsx_types.into_bytes())
            .is_err()
    );
    assert_eq!(office.format(), OfficeFormat::Docx);
}

#[test]
fn spreadsheet_output_budget_is_global_and_counts_resolved_values() {
    let worksheet =
        br#"<worksheet><sheetData><row><c r="A1" t="s"><v>0</v></c></row></sheetData></worksheet>"#;
    let mut budget = TextExtractionBudget {
        bytes: MAX_EXTRACTED_TEXT_BYTES - 1,
        items: 12,
    };
    let error = extract_sheet_cells(worksheet, &["expanded".to_string()], &mut budget)
        .expect_err("resolved shared string must count against the global output budget");
    assert!(error.to_string().contains("extracted text"));
}

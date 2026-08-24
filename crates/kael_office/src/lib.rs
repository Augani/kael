#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

use std::{
    collections::{BTreeMap, HashMap},
    io::{Cursor, Read as _, Write as _},
};

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use quick_xml::{
    Reader, XmlVersion,
    events::{BytesStart, Event},
};
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter, write::SimpleFileOptions};

const MAX_PACKAGE_BYTES: usize = 256 * 1024 * 1024;
const MAX_PARTS: usize = 65_536;
const MAX_PART_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PART_NAME_BYTES: usize = 1_024;
const MAX_COMPRESSION_RATIO: u64 = 500;
const COMPRESSION_RATIO_SLACK: u64 = 1024 * 1024;
const MAX_XML_EVENTS: usize = 5_000_000;
const MAX_XML_DEPTH: usize = 256;
const MAX_EXTRACTED_TEXT_BYTES: usize = 64 * 1024 * 1024;
const MAX_TEXT_ITEMS: usize = 1_000_000;

const CONTENT_TYPES_PART: &str = "[Content_Types].xml";
const CORE_PROPERTIES_PART: &str = "docProps/core.xml";
const DOCX_MAIN_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const XLSX_MAIN_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
const PPTX_MAIN_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";

/// The detected OOXML application format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OfficeFormat {
    /// A WordprocessingML `.docx` package.
    Docx,
    /// A SpreadsheetML `.xlsx` package.
    Xlsx,
    /// A PresentationML `.pptx` package.
    Pptx,
}

/// Metadata for one OPC package part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficePart {
    /// The validated package-relative part name without a leading slash.
    pub name: String,
    /// The expanded byte length.
    pub byte_len: usize,
    /// The MIME content type resolved from `[Content_Types].xml`, when known.
    pub content_type: Option<String>,
}

/// Dublin Core and OPC core properties from `docProps/core.xml`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoreProperties {
    /// Document title.
    pub title: Option<String>,
    /// Document subject.
    pub subject: Option<String>,
    /// Primary creator.
    pub creator: Option<String>,
    /// Keywords as stored by the producer.
    pub keywords: Option<String>,
    /// Description or comments.
    pub description: Option<String>,
    /// Last modifying user.
    pub last_modified_by: Option<String>,
    /// Producer revision string.
    pub revision: Option<String>,
    /// Creation timestamp string.
    pub created: Option<String>,
    /// Modification timestamp string.
    pub modified: Option<String>,
    /// Document category.
    pub category: Option<String>,
    /// Producer-defined content status.
    pub content_status: Option<String>,
}

/// One OPC relationship entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageRelationship {
    /// Relationship identifier, such as `rId1`.
    pub id: String,
    /// Relationship type URI.
    pub relationship_type: String,
    /// Target URI exactly as stored in the relationship part.
    pub target: String,
    /// Target mode, commonly `External`; absent means an internal package target.
    pub target_mode: Option<String>,
}

/// Extracted Word paragraph content.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentText {
    /// Paragraphs in document order, including paragraphs nested in tables.
    pub paragraphs: Vec<String>,
}

/// One extracted spreadsheet cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellText {
    /// A1-style cell reference when present.
    pub reference: String,
    /// Cached or literal display source value; formulas are not evaluated.
    pub value: String,
}

/// Extracted cells for one worksheet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SheetText {
    /// Worksheet display name, or a stable part-derived fallback.
    pub name: String,
    /// Non-empty cells in document order.
    pub cells: Vec<CellText>,
}

/// Extracted workbook content.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpreadsheetText {
    /// Worksheets in workbook order when the workbook relationships are valid.
    pub sheets: Vec<SheetText>,
}

/// Extracted content for one slide.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlideText {
    /// The package part that contains the slide.
    pub part_name: String,
    /// DrawingML paragraphs in slide order.
    pub paragraphs: Vec<String>,
}

/// Extracted presentation content.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PresentationText {
    /// Slides in presentation order when presentation relationships are valid.
    pub slides: Vec<SlideText>,
}

/// Format-specific text extracted without performing layout or calculation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfficeText {
    /// WordprocessingML paragraph text.
    Document(DocumentText),
    /// SpreadsheetML cached and literal cell text.
    Spreadsheet(SpreadsheetText),
    /// PresentationML slide paragraph text.
    Presentation(PresentationText),
}

#[derive(Debug, Clone, Default)]
struct ContentTypes {
    defaults: BTreeMap<String, String>,
    overrides: BTreeMap<String, String>,
}

impl ContentTypes {
    fn for_part(&self, name: &str) -> Option<&str> {
        self.overrides.get(name).map(String::as_str).or_else(|| {
            name.rsplit_once('.')
                .and_then(|(_, extension)| self.defaults.get(&extension.to_ascii_lowercase()))
                .map(String::as_str)
        })
    }
}

/// A bounded in-memory OOXML/OPC package shared by desktop and browser builds.
#[derive(Debug, Clone)]
pub struct OfficePackage {
    format: OfficeFormat,
    main_part: String,
    parts: BTreeMap<String, Vec<u8>>,
    content_types: ContentTypes,
    expanded_bytes: u64,
}

impl OfficePackage {
    /// Parses a `.docx`, `.xlsx`, or `.pptx` from bounded ZIP bytes.
    pub fn open(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_PACKAGE_BYTES,
            "Office package exceeds the {MAX_PACKAGE_BYTES} byte limit"
        );
        preflight_zip_entry_count(bytes)?;
        let mut archive = ZipArchive::new(Cursor::new(bytes)).context("invalid Office ZIP")?;
        ensure!(
            archive.len() <= MAX_PARTS,
            "Office package contains more than {MAX_PARTS} parts"
        );

        let mut parts = BTreeMap::new();
        let mut expanded_bytes = 0u64;
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .with_context(|| format!("failed to read ZIP entry {index}"))?;
            let raw_name = std::str::from_utf8(entry.name_raw())
                .context("Office ZIP part name is not UTF-8")?
                .to_string();
            ensure!(
                raw_name == entry.name(),
                "Office ZIP part name is ambiguously encoded"
            );
            if entry.is_dir() {
                let directory_name = raw_name.trim_end_matches('/');
                validate_part_name(directory_name)?;
                continue;
            }
            validate_part_name(&raw_name)?;
            ensure!(
                !entry.encrypted(),
                "encrypted Office ZIP parts are unsupported"
            );
            ensure!(
                entry.size() <= MAX_PART_BYTES,
                "Office part `{raw_name}` exceeds the {MAX_PART_BYTES} byte expanded limit"
            );
            validate_compression_ratio(&raw_name, entry.compressed_size(), entry.size())?;
            expanded_bytes = expanded_bytes
                .checked_add(entry.size())
                .ok_or_else(|| anyhow!("Office expanded byte count overflow"))?;
            ensure!(
                expanded_bytes <= MAX_EXPANDED_BYTES,
                "Office package exceeds the {MAX_EXPANDED_BYTES} byte expanded limit"
            );

            let capacity = usize::try_from(entry.size())
                .map_err(|_| anyhow!("Office part is too large for this platform"))?;
            let mut part_bytes = Vec::with_capacity(capacity);
            (&mut entry)
                .take(MAX_PART_BYTES + 1)
                .read_to_end(&mut part_bytes)
                .with_context(|| format!("failed to expand Office part `{raw_name}`"))?;
            ensure!(
                u64::try_from(part_bytes.len()).unwrap_or(u64::MAX) <= MAX_PART_BYTES,
                "Office part `{raw_name}` exceeded its expanded limit while reading"
            );
            ensure!(
                u64::try_from(part_bytes.len()).unwrap_or(u64::MAX) == entry.size(),
                "Office part `{raw_name}` expanded to an unexpected byte length"
            );
            ensure!(
                !parts.contains_key(&raw_name),
                "duplicate Office part `{raw_name}`"
            );
            parts.insert(raw_name, part_bytes);
        }

        let manifest = parts
            .get(CONTENT_TYPES_PART)
            .ok_or_else(|| anyhow!("Office package is missing `{CONTENT_TYPES_PART}`"))?;
        let content_types = parse_content_types(manifest)?;
        let (format, main_part) = detect_format(&content_types, &parts)?;
        Ok(Self {
            format,
            main_part,
            parts,
            content_types,
            expanded_bytes,
        })
    }

    /// Returns the detected application format.
    pub const fn format(&self) -> OfficeFormat {
        self.format
    }

    /// Returns the main document, workbook, or presentation part name.
    pub fn main_part_name(&self) -> &str {
        &self.main_part
    }

    /// Returns stable metadata for every package part in lexical order.
    pub fn parts(&self) -> Vec<OfficePart> {
        self.parts
            .iter()
            .map(|(name, bytes)| OfficePart {
                name: name.clone(),
                byte_len: bytes.len(),
                content_type: self.content_types.for_part(name).map(str::to_string),
            })
            .collect()
    }

    /// Returns a raw part by package-relative name.
    ///
    /// A single leading slash, as used by OPC content-type overrides, is accepted.
    pub fn read_part(&self, name: &str) -> Result<Option<&[u8]>> {
        let name = normalize_api_part_name(name)?;
        Ok(self.parts.get(name).map(Vec::as_slice))
    }

    /// Replaces an existing raw part while preserving all other parts byte for byte.
    pub fn replace_part(&mut self, name: &str, bytes: impl Into<Vec<u8>>) -> Result<Vec<u8>> {
        let name = normalize_api_part_name(name)?.to_string();
        ensure!(
            self.parts.contains_key(&name),
            "Office part `{name}` does not exist"
        );
        let bytes = bytes.into();
        self.validate_replacement(&name, &bytes)?;

        if name == CONTENT_TYPES_PART {
            let next_content_types = parse_content_types(&bytes)?;
            let (next_format, next_main_part) = detect_format(&next_content_types, &self.parts)?;
            ensure!(
                next_format == self.format,
                "replacing `[Content_Types].xml` cannot change the package format"
            );
            self.content_types = next_content_types;
            self.main_part = next_main_part;
        }

        let previous = self
            .parts
            .insert(name, bytes)
            .ok_or_else(|| anyhow!("Office part disappeared during replacement"))?;
        self.recalculate_expanded_bytes()?;
        Ok(previous)
    }

    /// Adds a new raw part.
    ///
    /// Callers must separately update content types and relationships as required by OPC.
    pub fn add_part(&mut self, name: &str, bytes: impl Into<Vec<u8>>) -> Result<()> {
        let name = normalize_api_part_name(name)?.to_string();
        ensure!(
            !self.parts.contains_key(&name),
            "Office part `{name}` already exists"
        );
        ensure!(
            self.parts.len() < MAX_PARTS,
            "Office package contains too many parts"
        );
        ensure!(
            name != CONTENT_TYPES_PART,
            "`[Content_Types].xml` already exists and must be replaced, not added"
        );
        let bytes = bytes.into();
        self.validate_replacement(&name, &bytes)?;
        self.parts.insert(name, bytes);
        self.recalculate_expanded_bytes()
    }

    /// Removes a raw part and returns its bytes.
    ///
    /// The content-types manifest and detected main part cannot be removed. Callers must update
    /// any relationship that referenced another removed part.
    pub fn remove_part(&mut self, name: &str) -> Result<Vec<u8>> {
        let name = normalize_api_part_name(name)?.to_string();
        ensure!(
            name != CONTENT_TYPES_PART && name != self.main_part,
            "cannot remove a package manifest or main Office part"
        );
        let removed = self
            .parts
            .remove(&name)
            .ok_or_else(|| anyhow!("Office part `{name}` does not exist"))?;
        self.recalculate_expanded_bytes()?;
        Ok(removed)
    }

    /// Returns parsed package or part relationships.
    ///
    /// Pass `None` for `_rels/.rels`, or a source part such as `xl/workbook.xml`.
    pub fn relationships(&self, source_part: Option<&str>) -> Result<Vec<PackageRelationship>> {
        let relationship_part = relationship_part_name(source_part)?;
        let Some(bytes) = self.parts.get(&relationship_part) else {
            return Ok(Vec::new());
        };
        parse_relationships(bytes)
    }

    /// Resolves an internal relationship target to a validated package part name.
    ///
    /// External relationships return `Ok(None)` and must be handled as untrusted URLs by the app.
    pub fn resolve_relationship_target(
        &self,
        source_part: Option<&str>,
        relationship: &PackageRelationship,
    ) -> Result<Option<String>> {
        if relationship
            .target_mode
            .as_deref()
            .is_some_and(|mode| mode.eq_ignore_ascii_case("external"))
        {
            return Ok(None);
        }
        let resolved = resolve_internal_target(source_part, &relationship.target)?;
        validate_part_name(&resolved)?;
        Ok(Some(resolved))
    }

    /// Returns the parsed package core properties, or an empty model when absent.
    pub fn core_properties(&self) -> Result<CoreProperties> {
        self.parts.get(CORE_PROPERTIES_PART).map_or_else(
            || Ok(CoreProperties::default()),
            |bytes| parse_core_properties(bytes),
        )
    }

    /// Extracts common paragraph, cell, or slide text without layout or formula evaluation.
    pub fn extract_text(&self) -> Result<OfficeText> {
        match self.format {
            OfficeFormat::Docx => extract_docx(self).map(OfficeText::Document),
            OfficeFormat::Xlsx => extract_xlsx(self).map(OfficeText::Spreadsheet),
            OfficeFormat::Pptx => extract_pptx(self).map(OfficeText::Presentation),
        }
    }

    /// Writes deterministic ZIP bytes with lexical part order, fixed timestamps and permissions,
    /// and a fixed Deflate level.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        ensure!(
            self.parts.contains_key(&self.main_part),
            "Office main part is missing"
        );
        let manifest = self
            .parts
            .get(CONTENT_TYPES_PART)
            .ok_or_else(|| anyhow!("Office content-types manifest is missing"))?;
        let manifest_types = parse_content_types(manifest)?;
        let (format, main_part) = detect_format(&manifest_types, &self.parts)?;
        ensure!(
            format == self.format && main_part == self.main_part,
            "Office package manifest no longer matches its detected format"
        );

        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(6))
            .last_modified_time(DateTime::default())
            .unix_permissions(0o644)
            .large_file(false);
        for (name, bytes) in &self.parts {
            writer
                .start_file(name, options)
                .with_context(|| format!("failed to start Office ZIP part `{name}`"))?;
            writer
                .write_all(bytes)
                .with_context(|| format!("failed to write Office ZIP part `{name}`"))?;
        }
        let bytes = writer
            .finish()
            .context("failed to finish Office ZIP")?
            .into_inner();
        ensure!(
            bytes.len() <= MAX_PACKAGE_BYTES,
            "exported Office package exceeds the {MAX_PACKAGE_BYTES} byte limit"
        );
        Ok(bytes)
    }

    fn validate_replacement(&self, name: &str, bytes: &[u8]) -> Result<()> {
        validate_part_name(name)?;
        ensure!(
            u64::try_from(bytes.len()).unwrap_or(u64::MAX) <= MAX_PART_BYTES,
            "Office part `{name}` exceeds the {MAX_PART_BYTES} byte limit"
        );
        let previous = self.parts.get(name).map_or(0, |part| part.len());
        let next = self
            .expanded_bytes
            .checked_sub(u64::try_from(previous).unwrap_or(u64::MAX))
            .and_then(|total| total.checked_add(u64::try_from(bytes.len()).ok()?))
            .ok_or_else(|| anyhow!("Office expanded byte count overflow"))?;
        ensure!(
            next <= MAX_EXPANDED_BYTES,
            "Office package exceeds the expanded byte limit"
        );
        Ok(())
    }

    fn recalculate_expanded_bytes(&mut self) -> Result<()> {
        self.expanded_bytes = self.parts.values().try_fold(0u64, |total, bytes| {
            total
                .checked_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
                .ok_or_else(|| anyhow!("Office expanded byte count overflow"))
        })?;
        ensure!(
            self.expanded_bytes <= MAX_EXPANDED_BYTES,
            "Office package exceeds the expanded byte limit"
        );
        Ok(())
    }
}

fn preflight_zip_entry_count(bytes: &[u8]) -> Result<()> {
    const EOCD_SIGNATURE: &[u8; 4] = b"PK\x05\x06";
    const ZIP64_LOCATOR_SIGNATURE: &[u8; 4] = b"PK\x06\x07";
    const ZIP64_EOCD_SIGNATURE: &[u8; 4] = b"PK\x06\x06";
    const MAX_EOCD_SEARCH: usize = 65_535 + 22;

    let search_start = bytes.len().saturating_sub(MAX_EOCD_SEARCH);
    let eocd = (search_start..bytes.len().saturating_sub(21))
        .rev()
        .find(|offset| {
            bytes.get(*offset..offset + 4) == Some(EOCD_SIGNATURE)
                && read_u16(bytes, offset + 20)
                    .is_some_and(|comment| offset + 22 + usize::from(comment) == bytes.len())
        })
        .ok_or_else(|| anyhow!("Office ZIP end-of-central-directory record is missing"))?;
    ensure!(
        read_u16(bytes, eocd + 4) == Some(0) && read_u16(bytes, eocd + 6) == Some(0),
        "multi-disk Office ZIP packages are unsupported"
    );
    let entries =
        read_u16(bytes, eocd + 10).ok_or_else(|| anyhow!("truncated Office ZIP directory"))?;
    let entry_count = if entries != u16::MAX {
        u64::from(entries)
    } else {
        ensure!(eocd >= 20, "Office ZIP64 locator is missing");
        let locator = eocd - 20;
        ensure!(
            bytes.get(locator..locator + 4) == Some(ZIP64_LOCATOR_SIGNATURE),
            "Office ZIP64 locator is missing"
        );
        let record_offset = read_u64(bytes, locator + 8)
            .ok_or_else(|| anyhow!("truncated Office ZIP64 locator"))?;
        let record_offset = usize::try_from(record_offset)
            .map_err(|_| anyhow!("Office ZIP64 record offset is too large"))?;
        ensure!(
            bytes.get(record_offset..record_offset + 4) == Some(ZIP64_EOCD_SIGNATURE),
            "Office ZIP64 directory record is missing"
        );
        read_u64(bytes, record_offset + 32)
            .ok_or_else(|| anyhow!("truncated Office ZIP64 directory record"))?
    };
    ensure!(
        entry_count <= MAX_PARTS as u64,
        "Office package contains more than {MAX_PARTS} ZIP entries"
    );
    Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

fn validate_compression_ratio(name: &str, compressed: u64, expanded: u64) -> Result<()> {
    if expanded <= COMPRESSION_RATIO_SLACK {
        return Ok(());
    }
    ensure!(
        compressed > 0,
        "Office part `{name}` has an invalid compressed size"
    );
    let permitted = compressed
        .saturating_mul(MAX_COMPRESSION_RATIO)
        .saturating_add(COMPRESSION_RATIO_SLACK);
    ensure!(
        expanded <= permitted,
        "Office part `{name}` exceeds the {MAX_COMPRESSION_RATIO}:1 compression-ratio limit"
    );
    Ok(())
}

fn normalize_api_part_name(name: &str) -> Result<&str> {
    let name = name.strip_prefix('/').unwrap_or(name);
    validate_part_name(name)?;
    Ok(name)
}

fn validate_part_name(name: &str) -> Result<()> {
    ensure!(!name.is_empty(), "Office part name cannot be empty");
    ensure!(
        name.len() <= MAX_PART_NAME_BYTES,
        "Office part name exceeds the {MAX_PART_NAME_BYTES} byte limit"
    );
    ensure!(
        !name.starts_with('/') && !name.ends_with('/') && !name.contains('\\'),
        "Office part name `{name}` is not package-relative"
    );
    ensure!(
        !name.chars().any(char::is_control),
        "Office part name contains control characters"
    );
    ensure!(
        name.split('/').all(|component| {
            !component.is_empty()
                && component != "."
                && component != ".."
                && !component.contains(':')
        }),
        "Office part name `{name}` contains an unsafe path component"
    );
    Ok(())
}

fn parse_content_types(bytes: &[u8]) -> Result<ContentTypes> {
    let mut reader = xml_reader(bytes);
    let mut buffer = Vec::new();
    let mut budget = XmlBudget::default();
    let mut types = ContentTypes::default();
    let mut saw_root = false;
    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .context("invalid content-types XML")?;
        budget.observe(&event)?;
        match event {
            Event::Start(ref start) | Event::Empty(ref start) => {
                match local_name(start.name().as_ref()) {
                    b"Types" => saw_root = true,
                    b"Default" => {
                        let extension = required_attribute(&reader, start, b"Extension")?;
                        let content_type = required_attribute(&reader, start, b"ContentType")?;
                        ensure!(
                            extension.len() <= 255 && content_type.len() <= 4_096,
                            "content-type declaration is too large"
                        );
                        types
                            .defaults
                            .insert(extension.to_ascii_lowercase(), content_type);
                    }
                    b"Override" => {
                        let part_name = required_attribute(&reader, start, b"PartName")?;
                        let part_name = normalize_api_part_name(&part_name)?.to_string();
                        let content_type = required_attribute(&reader, start, b"ContentType")?;
                        ensure!(content_type.len() <= 4_096, "content type is too large");
                        types.overrides.insert(part_name, content_type);
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    ensure!(saw_root, "content-types XML has no `Types` root");
    Ok(types)
}

fn detect_format(
    content_types: &ContentTypes,
    parts: &BTreeMap<String, Vec<u8>>,
) -> Result<(OfficeFormat, String)> {
    let mut detected = content_types
        .overrides
        .iter()
        .filter_map(|(part, content_type)| {
            let format = match content_type.as_str() {
                DOCX_MAIN_TYPE => OfficeFormat::Docx,
                XLSX_MAIN_TYPE => OfficeFormat::Xlsx,
                PPTX_MAIN_TYPE => OfficeFormat::Pptx,
                _ => return None,
            };
            Some((format, part.clone()))
        });
    let first = detected
        .next()
        .ok_or_else(|| anyhow!("package is not a supported `.docx`, `.xlsx`, or `.pptx`"))?;
    ensure!(
        detected.next().is_none(),
        "Office package declares multiple main parts"
    );
    ensure!(
        parts.contains_key(&first.1),
        "declared Office main part is missing"
    );
    Ok(first)
}

fn relationship_part_name(source_part: Option<&str>) -> Result<String> {
    let Some(source) = source_part else {
        return Ok("_rels/.rels".to_string());
    };
    let source = normalize_api_part_name(source)?;
    let (parent, file) = source.rsplit_once('/').map_or(("", source), |pair| pair);
    if parent.is_empty() {
        Ok(format!("_rels/{file}.rels"))
    } else {
        Ok(format!("{parent}/_rels/{file}.rels"))
    }
}

fn parse_relationships(bytes: &[u8]) -> Result<Vec<PackageRelationship>> {
    let mut reader = xml_reader(bytes);
    let mut buffer = Vec::new();
    let mut budget = XmlBudget::default();
    let mut relationships = Vec::new();
    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .context("invalid relationships XML")?;
        budget.observe(&event)?;
        if let Event::Start(ref start) | Event::Empty(ref start) = event
            && local_name(start.name().as_ref()) == b"Relationship"
        {
            ensure!(
                relationships.len() < MAX_PARTS,
                "too many package relationships"
            );
            let relationship = PackageRelationship {
                id: required_attribute(&reader, start, b"Id")?,
                relationship_type: required_attribute(&reader, start, b"Type")?,
                target: required_attribute(&reader, start, b"Target")?,
                target_mode: optional_attribute(&reader, start, b"TargetMode")?,
            };
            ensure!(
                relationship.id.len() <= 4_096
                    && relationship.relationship_type.len() <= 16 * 1024
                    && relationship.target.len() <= 16 * 1024,
                "package relationship field is too large"
            );
            ensure!(
                !relationship.target.chars().any(char::is_control),
                "package relationship target contains control characters"
            );
            relationships.push(relationship);
        }
        if matches!(event, Event::Eof) {
            break;
        }
        buffer.clear();
    }
    Ok(relationships)
}

fn resolve_internal_target(source_part: Option<&str>, target: &str) -> Result<String> {
    ensure!(!target.is_empty(), "relationship target cannot be empty");
    ensure!(
        !target.contains('\\'),
        "relationship target uses an unsafe separator"
    );
    ensure!(
        !target.contains('?') && !target.contains('#') && !target.contains('%'),
        "encoded, query, and fragment relationship targets require application handling"
    );
    let mut components = Vec::<&str>::new();
    if !target.starts_with('/')
        && let Some(source) = source_part
    {
        let source = normalize_api_part_name(source)?;
        if let Some((parent, _)) = source.rsplit_once('/') {
            components.extend(parent.split('/'));
        }
    }
    for component in target.trim_start_matches('/').split('/') {
        match component {
            "" | "." => {}
            ".." => {
                ensure!(
                    components.pop().is_some(),
                    "relationship target escapes the package"
                );
            }
            component => {
                ensure!(
                    !component.contains(':'),
                    "relationship target contains a URI scheme"
                );
                components.push(component);
            }
        }
    }
    let resolved = components.join("/");
    validate_part_name(&resolved)?;
    Ok(resolved)
}

fn parse_core_properties(bytes: &[u8]) -> Result<CoreProperties> {
    #[derive(Clone, Copy)]
    enum Field {
        Title,
        Subject,
        Creator,
        Keywords,
        Description,
        LastModifiedBy,
        Revision,
        Created,
        Modified,
        Category,
        ContentStatus,
    }
    let mut reader = xml_reader(bytes);
    let mut buffer = Vec::new();
    let mut budget = XmlBudget::default();
    let mut current = None;
    let mut value = String::new();
    let mut properties = CoreProperties::default();
    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .context("invalid core-properties XML")?;
        budget.observe(&event)?;
        match event {
            Event::Start(ref start) => {
                current = match local_name(start.name().as_ref()) {
                    b"title" => Some(Field::Title),
                    b"subject" => Some(Field::Subject),
                    b"creator" => Some(Field::Creator),
                    b"keywords" => Some(Field::Keywords),
                    b"description" => Some(Field::Description),
                    b"lastModifiedBy" => Some(Field::LastModifiedBy),
                    b"revision" => Some(Field::Revision),
                    b"created" => Some(Field::Created),
                    b"modified" => Some(Field::Modified),
                    b"category" => Some(Field::Category),
                    b"contentStatus" => Some(Field::ContentStatus),
                    _ => None,
                };
                if current.is_some() {
                    value.clear();
                }
            }
            Event::Text(ref text) if current.is_some() => {
                append_xml_text(&mut value, text, MAX_EXTRACTED_TEXT_BYTES)?;
            }
            Event::GeneralRef(ref reference) if current.is_some() => {
                let decoded = decode_general_reference(reference)?;
                append_decoded(&mut value, &decoded, MAX_EXTRACTED_TEXT_BYTES)?;
            }
            Event::CData(ref text) if current.is_some() => {
                append_decoded(
                    &mut value,
                    text.decode()?.as_ref(),
                    MAX_EXTRACTED_TEXT_BYTES,
                )?;
            }
            Event::End(ref end) if current.is_some() => {
                let end_field = match local_name(end.name().as_ref()) {
                    b"title" => Some(Field::Title),
                    b"subject" => Some(Field::Subject),
                    b"creator" => Some(Field::Creator),
                    b"keywords" => Some(Field::Keywords),
                    b"description" => Some(Field::Description),
                    b"lastModifiedBy" => Some(Field::LastModifiedBy),
                    b"revision" => Some(Field::Revision),
                    b"created" => Some(Field::Created),
                    b"modified" => Some(Field::Modified),
                    b"category" => Some(Field::Category),
                    b"contentStatus" => Some(Field::ContentStatus),
                    _ => None,
                };
                if end_field.is_some() {
                    let stored = (!value.is_empty()).then(|| value.clone());
                    match current.take().expect("checked above") {
                        Field::Title => properties.title = stored,
                        Field::Subject => properties.subject = stored,
                        Field::Creator => properties.creator = stored,
                        Field::Keywords => properties.keywords = stored,
                        Field::Description => properties.description = stored,
                        Field::LastModifiedBy => properties.last_modified_by = stored,
                        Field::Revision => properties.revision = stored,
                        Field::Created => properties.created = stored,
                        Field::Modified => properties.modified = stored,
                        Field::Category => properties.category = stored,
                        Field::ContentStatus => properties.content_status = stored,
                    }
                    value.clear();
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(properties)
}

fn extract_docx(package: &OfficePackage) -> Result<DocumentText> {
    let bytes = package
        .parts
        .get(&package.main_part)
        .ok_or_else(|| anyhow!("Word main document part is missing"))?;
    let mut extraction_budget = TextExtractionBudget::default();
    let paragraphs = extract_paragraphs(bytes, true, &mut extraction_budget)?;
    Ok(DocumentText { paragraphs })
}

fn extract_xlsx(package: &OfficePackage) -> Result<SpreadsheetText> {
    let shared_strings = package
        .parts
        .get("xl/sharedStrings.xml")
        .map_or_else(|| Ok(Vec::new()), |bytes| extract_shared_strings(bytes))?;
    let mut sheets = workbook_sheet_parts(package)?;
    if sheets.is_empty() {
        sheets = numbered_parts(&package.parts, "xl/worksheets/sheet", ".xml")
            .into_iter()
            .enumerate()
            .map(|(index, part)| (format!("Sheet {}", index + 1), part))
            .collect();
    }
    let mut extracted = Vec::with_capacity(sheets.len());
    let mut extraction_budget = TextExtractionBudget::default();
    for (name, part) in sheets {
        let bytes = package
            .parts
            .get(&part)
            .ok_or_else(|| anyhow!("worksheet part `{part}` is missing"))?;
        extracted.push(SheetText {
            name,
            cells: extract_sheet_cells(bytes, &shared_strings, &mut extraction_budget)?,
        });
    }
    Ok(SpreadsheetText { sheets: extracted })
}

fn extract_pptx(package: &OfficePackage) -> Result<PresentationText> {
    let mut parts = presentation_slide_parts(package)?;
    if parts.is_empty() {
        parts = numbered_parts(&package.parts, "ppt/slides/slide", ".xml");
    }
    let mut slides = Vec::with_capacity(parts.len());
    let mut extraction_budget = TextExtractionBudget::default();
    for part_name in parts {
        let bytes = package
            .parts
            .get(&part_name)
            .ok_or_else(|| anyhow!("slide part `{part_name}` is missing"))?;
        slides.push(SlideText {
            part_name,
            paragraphs: extract_paragraphs(bytes, false, &mut extraction_budget)?,
        });
    }
    Ok(PresentationText { slides })
}

fn workbook_sheet_parts(package: &OfficePackage) -> Result<Vec<(String, String)>> {
    let relationships = package.relationships(Some(&package.main_part))?;
    let targets = relationships
        .iter()
        .filter_map(|relationship| {
            package
                .resolve_relationship_target(Some(&package.main_part), relationship)
                .transpose()
                .map(|target| target.map(|target| (relationship.id.clone(), target)))
        })
        .collect::<Result<HashMap<_, _>>>()?;
    let workbook = package
        .parts
        .get(&package.main_part)
        .ok_or_else(|| anyhow!("workbook main part is missing"))?;
    let mut reader = xml_reader(workbook);
    let mut buffer = Vec::new();
    let mut budget = XmlBudget::default();
    let mut sheets = Vec::new();
    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .context("invalid workbook XML")?;
        budget.observe(&event)?;
        if let Event::Start(ref start) | Event::Empty(ref start) = event
            && local_name(start.name().as_ref()) == b"sheet"
        {
            let name = required_attribute(&reader, start, b"name")?;
            let id = required_relationship_id(&reader, start)?;
            if let Some(target) = targets.get(&id) {
                sheets.push((name, target.clone()));
            }
        }
        if matches!(event, Event::Eof) {
            break;
        }
        buffer.clear();
    }
    Ok(sheets)
}

fn presentation_slide_parts(package: &OfficePackage) -> Result<Vec<String>> {
    let relationships = package.relationships(Some(&package.main_part))?;
    let targets = relationships
        .iter()
        .filter_map(|relationship| {
            package
                .resolve_relationship_target(Some(&package.main_part), relationship)
                .transpose()
                .map(|target| target.map(|target| (relationship.id.clone(), target)))
        })
        .collect::<Result<HashMap<_, _>>>()?;
    let presentation = package
        .parts
        .get(&package.main_part)
        .ok_or_else(|| anyhow!("presentation main part is missing"))?;
    let mut reader = xml_reader(presentation);
    let mut buffer = Vec::new();
    let mut budget = XmlBudget::default();
    let mut slides = Vec::new();
    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .context("invalid presentation XML")?;
        budget.observe(&event)?;
        if let Event::Start(ref start) | Event::Empty(ref start) = event
            && local_name(start.name().as_ref()) == b"sldId"
        {
            let id = required_relationship_id(&reader, start)?;
            if let Some(target) = targets.get(&id) {
                slides.push(target.clone());
            }
        }
        if matches!(event, Event::Eof) {
            break;
        }
        buffer.clear();
    }
    Ok(slides)
}

fn extract_paragraphs(
    bytes: &[u8],
    wordprocessing: bool,
    extraction_budget: &mut TextExtractionBudget,
) -> Result<Vec<String>> {
    let mut reader = xml_reader(bytes);
    let mut buffer = Vec::new();
    let mut budget = XmlBudget::default();
    let mut paragraphs = Vec::new();
    let mut paragraph = None::<String>;
    let mut in_text = false;
    let mut total_text = 0usize;
    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .context("invalid Office text XML")?;
        budget.observe(&event)?;
        match event {
            Event::Start(ref start) if local_name(start.name().as_ref()) == b"p" => {
                ensure!(
                    paragraph.is_none(),
                    "nested Office paragraphs are unsupported"
                );
                paragraph = Some(String::new());
            }
            Event::Empty(ref start) if local_name(start.name().as_ref()) == b"p" => {
                extraction_budget.add_item(0, "Office paragraphs")?;
                paragraphs.push(String::new());
            }
            Event::Start(ref start) if local_name(start.name().as_ref()) == b"t" => {
                in_text = paragraph.is_some();
            }
            Event::Empty(ref start) if paragraph.is_some() && wordprocessing => {
                match local_name(start.name().as_ref()) {
                    b"tab" => append_bounded(
                        paragraph.as_mut().expect("checked above"),
                        "\t",
                        &mut total_text,
                    )?,
                    b"br" | b"cr" => append_bounded(
                        paragraph.as_mut().expect("checked above"),
                        "\n",
                        &mut total_text,
                    )?,
                    _ => {}
                }
            }
            Event::Text(ref text) if in_text => {
                let decoded = decode_xml_text(text)?;
                append_bounded(
                    paragraph
                        .as_mut()
                        .ok_or_else(|| anyhow!("text outside paragraph"))?,
                    &decoded,
                    &mut total_text,
                )?;
            }
            Event::GeneralRef(ref reference) if in_text => {
                let decoded = decode_general_reference(reference)?;
                append_bounded(
                    paragraph
                        .as_mut()
                        .ok_or_else(|| anyhow!("entity outside paragraph"))?,
                    &decoded,
                    &mut total_text,
                )?;
            }
            Event::CData(ref text) if in_text => {
                let decoded = text.decode()?;
                append_bounded(
                    paragraph
                        .as_mut()
                        .ok_or_else(|| anyhow!("text outside paragraph"))?,
                    decoded.as_ref(),
                    &mut total_text,
                )?;
            }
            Event::End(ref end) if local_name(end.name().as_ref()) == b"t" => in_text = false,
            Event::End(ref end) if local_name(end.name().as_ref()) == b"p" => {
                let paragraph = paragraph
                    .take()
                    .ok_or_else(|| anyhow!("paragraph end without start"))?;
                extraction_budget.add_item(paragraph.len(), "Office paragraphs")?;
                paragraphs.push(paragraph);
                in_text = false;
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    ensure!(paragraph.is_none(), "unterminated Office paragraph");
    Ok(paragraphs)
}

fn extract_shared_strings(bytes: &[u8]) -> Result<Vec<String>> {
    let mut reader = xml_reader(bytes);
    let mut buffer = Vec::new();
    let mut budget = XmlBudget::default();
    let mut strings = Vec::new();
    let mut current = None::<String>;
    let mut in_text = false;
    let mut total_text = 0usize;
    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .context("invalid shared-strings XML")?;
        budget.observe(&event)?;
        match event {
            Event::Start(ref start) if local_name(start.name().as_ref()) == b"si" => {
                ensure!(current.is_none(), "nested shared string");
                current = Some(String::new());
            }
            Event::Start(ref start) if local_name(start.name().as_ref()) == b"t" => {
                in_text = current.is_some();
            }
            Event::Text(ref text) if in_text => {
                let decoded = decode_xml_text(text)?;
                append_bounded(
                    current
                        .as_mut()
                        .ok_or_else(|| anyhow!("shared text outside item"))?,
                    &decoded,
                    &mut total_text,
                )?;
            }
            Event::GeneralRef(ref reference) if in_text => {
                let decoded = decode_general_reference(reference)?;
                append_bounded(
                    current
                        .as_mut()
                        .ok_or_else(|| anyhow!("shared entity outside item"))?,
                    &decoded,
                    &mut total_text,
                )?;
            }
            Event::CData(ref text) if in_text => {
                let decoded = text.decode()?;
                append_bounded(
                    current
                        .as_mut()
                        .ok_or_else(|| anyhow!("shared text outside item"))?,
                    decoded.as_ref(),
                    &mut total_text,
                )?;
            }
            Event::End(ref end) if local_name(end.name().as_ref()) == b"t" => in_text = false,
            Event::End(ref end) if local_name(end.name().as_ref()) == b"si" => {
                ensure!(strings.len() < MAX_TEXT_ITEMS, "too many shared strings");
                strings.push(
                    current
                        .take()
                        .ok_or_else(|| anyhow!("shared string end without start"))?,
                );
                in_text = false;
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(strings)
}

fn extract_sheet_cells(
    bytes: &[u8],
    shared_strings: &[String],
    extraction_budget: &mut TextExtractionBudget,
) -> Result<Vec<CellText>> {
    struct Cell {
        reference: String,
        kind: String,
        value: String,
    }
    let mut reader = xml_reader(bytes);
    let mut buffer = Vec::new();
    let mut budget = XmlBudget::default();
    let mut cells = Vec::new();
    let mut cell = None::<Cell>;
    let mut capture = false;
    let mut total_text = 0usize;
    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .context("invalid worksheet XML")?;
        budget.observe(&event)?;
        match event {
            Event::Start(ref start) if local_name(start.name().as_ref()) == b"c" => {
                ensure!(cell.is_none(), "nested spreadsheet cell");
                cell = Some(Cell {
                    reference: optional_attribute(&reader, start, b"r")?.unwrap_or_default(),
                    kind: optional_attribute(&reader, start, b"t")?.unwrap_or_default(),
                    value: String::new(),
                });
            }
            Event::Start(ref start) if local_name(start.name().as_ref()) == b"v" => {
                capture = cell.is_some();
            }
            Event::Start(ref start)
                if local_name(start.name().as_ref()) == b"t"
                    && cell.as_ref().is_some_and(|cell| cell.kind == "inlineStr") =>
            {
                capture = true;
            }
            Event::Text(ref text) if capture => {
                let decoded = decode_xml_text(text)?;
                append_bounded(
                    &mut cell
                        .as_mut()
                        .ok_or_else(|| anyhow!("cell text outside cell"))?
                        .value,
                    &decoded,
                    &mut total_text,
                )?;
            }
            Event::GeneralRef(ref reference) if capture => {
                let decoded = decode_general_reference(reference)?;
                append_bounded(
                    &mut cell
                        .as_mut()
                        .ok_or_else(|| anyhow!("cell entity outside cell"))?
                        .value,
                    &decoded,
                    &mut total_text,
                )?;
            }
            Event::CData(ref text) if capture => {
                let decoded = text.decode()?;
                append_bounded(
                    &mut cell
                        .as_mut()
                        .ok_or_else(|| anyhow!("cell text outside cell"))?
                        .value,
                    decoded.as_ref(),
                    &mut total_text,
                )?;
            }
            Event::End(ref end)
                if local_name(end.name().as_ref()) == b"v"
                    || local_name(end.name().as_ref()) == b"t" =>
            {
                capture = false;
            }
            Event::End(ref end) if local_name(end.name().as_ref()) == b"c" => {
                let mut finished = cell
                    .take()
                    .ok_or_else(|| anyhow!("cell end without start"))?;
                finished.value = match finished.kind.as_str() {
                    "s" => finished
                        .value
                        .parse::<usize>()
                        .ok()
                        .and_then(|index| shared_strings.get(index))
                        .cloned()
                        .ok_or_else(|| anyhow!("spreadsheet shared-string index is invalid"))?,
                    "b" if finished.value == "1" => "true".to_string(),
                    "b" if finished.value == "0" => "false".to_string(),
                    _ => finished.value,
                };
                if !finished.value.is_empty() {
                    extraction_budget.add_item(
                        finished
                            .reference
                            .len()
                            .saturating_add(finished.value.len()),
                        "spreadsheet cells",
                    )?;
                    cells.push(CellText {
                        reference: finished.reference,
                        value: finished.value,
                    });
                }
                capture = false;
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(cells)
}

fn numbered_parts(parts: &BTreeMap<String, Vec<u8>>, prefix: &str, suffix: &str) -> Vec<String> {
    let mut names = parts
        .keys()
        .filter_map(|name| {
            name.strip_prefix(prefix)
                .and_then(|rest| rest.strip_suffix(suffix))
                .and_then(|number| number.parse::<u64>().ok())
                .map(|number| (number, name.clone()))
        })
        .collect::<Vec<_>>();
    names.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    names.into_iter().map(|(_, name)| name).collect()
}

fn xml_reader(bytes: &[u8]) -> Reader<&[u8]> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    reader
}

#[derive(Default)]
struct XmlBudget {
    events: usize,
    depth: usize,
}

impl XmlBudget {
    fn observe(&mut self, event: &Event<'_>) -> Result<()> {
        self.events = self
            .events
            .checked_add(1)
            .ok_or_else(|| anyhow!("Office XML event count overflow"))?;
        ensure!(
            self.events <= MAX_XML_EVENTS,
            "Office XML contains too many events"
        );
        match event {
            Event::Start(_) => {
                self.depth = self
                    .depth
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("Office XML depth overflow"))?;
                ensure!(
                    self.depth <= MAX_XML_DEPTH,
                    "Office XML nesting is too deep"
                );
            }
            Event::End(_) => {
                self.depth = self
                    .depth
                    .checked_sub(1)
                    .ok_or_else(|| anyhow!("Office XML has an unmatched closing element"))?;
            }
            Event::DocType(_) => bail!("Office XML DTDs are unsupported"),
            Event::Eof => ensure!(self.depth == 0, "Office XML ended inside an element"),
            _ => {}
        }
        Ok(())
    }
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

fn optional_attribute(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    name: &[u8],
) -> Result<Option<String>> {
    for attribute in start.attributes().with_checks(true) {
        let attribute = attribute.context("invalid Office XML attribute")?;
        if local_name(attribute.key.as_ref()) == name {
            return Ok(Some(
                attribute
                    .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                    .context("invalid Office XML attribute value")?
                    .into_owned(),
            ));
        }
    }
    Ok(None)
}

fn required_attribute(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    name: &[u8],
) -> Result<String> {
    optional_attribute(reader, start, name)?.ok_or_else(|| {
        anyhow!(
            "Office XML element `{}` is missing attribute `{}`",
            String::from_utf8_lossy(local_name(start.name().as_ref())),
            String::from_utf8_lossy(name)
        )
    })
}

fn required_relationship_id(reader: &Reader<&[u8]>, start: &BytesStart<'_>) -> Result<String> {
    let mut fallback = None;
    for attribute in start.attributes().with_checks(true) {
        let attribute = attribute.context("invalid Office XML attribute")?;
        if local_name(attribute.key.as_ref()) != b"id" {
            continue;
        }
        let value = attribute
            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
            .context("invalid Office relationship id")?
            .into_owned();
        if attribute.key.as_ref().contains(&b':') {
            return Ok(value);
        }
        fallback = Some(value);
    }
    fallback.ok_or_else(|| anyhow!("Office XML element is missing its relationship id"))
}

fn decode_xml_text(text: &quick_xml::events::BytesText<'_>) -> Result<String> {
    Ok(text
        .xml10_content()
        .context("invalid Office XML text encoding")?
        .into_owned())
}

fn decode_general_reference(reference: &quick_xml::events::BytesRef<'_>) -> Result<String> {
    if let Some(character) = reference
        .resolve_char_ref()
        .context("invalid Office XML character reference")?
    {
        return Ok(character.to_string());
    }
    match reference.decode()?.as_ref() {
        "lt" => Ok("<".to_string()),
        "gt" => Ok(">".to_string()),
        "amp" => Ok("&".to_string()),
        "apos" => Ok("'".to_string()),
        "quot" => Ok("\"".to_string()),
        entity => bail!("custom Office XML entity `{entity}` is unsupported"),
    }
}

fn append_xml_text(
    destination: &mut String,
    text: &quick_xml::events::BytesText<'_>,
    limit: usize,
) -> Result<()> {
    let decoded = decode_xml_text(text)?;
    append_decoded(destination, &decoded, limit)
}

fn append_decoded(destination: &mut String, text: &str, limit: usize) -> Result<()> {
    ensure!(
        destination.len().saturating_add(text.len()) <= limit,
        "Office XML text exceeds the {limit} byte limit"
    );
    destination.push_str(text);
    Ok(())
}

fn append_bounded(destination: &mut String, text: &str, total: &mut usize) -> Result<()> {
    *total = total
        .checked_add(text.len())
        .ok_or_else(|| anyhow!("Office extracted text size overflow"))?;
    ensure!(
        *total <= MAX_EXTRACTED_TEXT_BYTES,
        "Office extracted text exceeds the {MAX_EXTRACTED_TEXT_BYTES} byte limit"
    );
    destination.push_str(text);
    Ok(())
}

#[derive(Default)]
struct TextExtractionBudget {
    bytes: usize,
    items: usize,
}

impl TextExtractionBudget {
    fn add_item(&mut self, bytes: usize, label: &str) -> Result<()> {
        self.items = self
            .items
            .checked_add(1)
            .ok_or_else(|| anyhow!("Office extracted item count overflow"))?;
        ensure!(self.items <= MAX_TEXT_ITEMS, "too many {label}");
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or_else(|| anyhow!("Office extracted text size overflow"))?;
        ensure!(
            self.bytes <= MAX_EXTRACTED_TEXT_BYTES,
            "Office extracted text exceeds the {MAX_EXTRACTED_TEXT_BYTES} byte limit"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests;

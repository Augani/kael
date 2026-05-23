/// Text and document editing engine for IDEs, notes apps, and chat composers.
///
/// Provides line-based text storage, multi-cursor editing, undo/redo history,
/// find/replace, code folding, and inline diagnostics.
use serde::{Deserialize, Serialize};

/// A position within a text buffer identified by line and column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TextPosition {
    /// Zero-based line index.
    pub line: usize,
    /// Zero-based column (character offset within the line).
    pub column: usize,
}

impl TextPosition {
    /// Create a new text position.
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }

    /// The origin position (0, 0).
    pub fn zero() -> Self {
        Self { line: 0, column: 0 }
    }
}

/// A range within a text buffer defined by a start and end position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TextRange {
    /// The inclusive start of the range.
    pub start: TextPosition,
    /// The exclusive end of the range.
    pub end: TextPosition,
}

impl TextRange {
    /// Create a new text range.
    pub fn new(start: TextPosition, end: TextPosition) -> Self {
        Self { start, end }
    }

    /// Whether this range is empty (start equals end).
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Return the range with start and end ordered so start <= end.
    pub fn ordered(&self) -> Self {
        if self.start <= self.end {
            *self
        } else {
            Self {
                start: self.end,
                end: self.start,
            }
        }
    }
}

/// Line-based text storage with versioning.
///
/// Stores text as a `Vec<String>` of lines, providing efficient line-level
/// access and modification. Each mutation increments the version counter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextBuffer {
    lines: Vec<String>,
    version: u64,
}

impl TextBuffer {
    /// Create a new empty text buffer with a single empty line.
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            version: 0,
        }
    }

    /// Create a text buffer from a string, splitting on newlines.
    pub fn from_str(text: &str) -> Self {
        let lines: Vec<String> = if text.is_empty() {
            vec![String::new()]
        } else {
            text.split('\n').map(String::from).collect()
        };
        Self { lines, version: 0 }
    }

    /// Return the full text content with lines joined by newlines.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Return a reference to the line at the given index.
    pub fn line(&self, line_idx: usize) -> Option<&str> {
        self.lines.get(line_idx).map(|s| s.as_str())
    }

    /// Return the number of lines in the buffer.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Return the total number of characters in the buffer, including newlines.
    pub fn len_chars(&self) -> usize {
        let line_chars: usize = self.lines.iter().map(|l| l.chars().count()).sum();
        let newline_count = if self.lines.len() > 1 {
            self.lines.len() - 1
        } else {
            0
        };
        line_chars + newline_count
    }

    /// Return the length (in characters) of the line at the given index.
    pub fn line_len(&self, line_idx: usize) -> Option<usize> {
        self.lines.get(line_idx).map(|l| l.chars().count())
    }

    /// Return the current version number. Incremented on each mutation.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Whether the buffer is empty (single empty line).
    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    /// Clamp a position to valid bounds within the buffer.
    fn clamp_position(&self, pos: TextPosition) -> TextPosition {
        if self.lines.is_empty() {
            return TextPosition::zero();
        }
        let line = pos.line.min(self.lines.len() - 1);
        let max_col = self.lines[line].chars().count();
        let column = pos.column.min(max_col);
        TextPosition::new(line, column)
    }

    /// Insert text at the given position. Handles multi-line insertions.
    pub fn insert(&mut self, position: TextPosition, text: &str) {
        let pos = self.clamp_position(position);

        if text.is_empty() {
            return;
        }

        let current_line = &self.lines[pos.line];
        let byte_offset = char_to_byte_offset(current_line, pos.column);
        let before = &current_line[..byte_offset];
        let after = &current_line[byte_offset..];

        let insert_lines: Vec<&str> = text.split('\n').collect();

        if insert_lines.len() == 1 {
            let mut new_line =
                String::with_capacity(before.len() + insert_lines[0].len() + after.len());
            new_line.push_str(before);
            new_line.push_str(insert_lines[0]);
            new_line.push_str(after);
            self.lines[pos.line] = new_line;
        } else {
            let first = format!("{}{}", before, insert_lines[0]);
            let last = format!("{}{}", insert_lines[insert_lines.len() - 1], after);

            let mut new_lines = Vec::with_capacity(insert_lines.len());
            new_lines.push(first);
            for segment in &insert_lines[1..insert_lines.len() - 1] {
                new_lines.push(segment.to_string());
            }
            new_lines.push(last);

            self.lines.splice(pos.line..=pos.line, new_lines);
        }

        self.version += 1;
    }

    /// Delete text within the given range.
    pub fn delete(&mut self, range: TextRange) {
        let range = range.ordered();
        let start = self.clamp_position(range.start);
        let end = self.clamp_position(range.end);

        if start == end {
            return;
        }

        if start.line == end.line {
            let line = &self.lines[start.line];
            let start_byte = char_to_byte_offset(line, start.column);
            let end_byte = char_to_byte_offset(line, end.column);
            let mut new_line = String::with_capacity(line.len() - (end_byte - start_byte));
            new_line.push_str(&line[..start_byte]);
            new_line.push_str(&line[end_byte..]);
            self.lines[start.line] = new_line;
        } else {
            let start_line = &self.lines[start.line];
            let end_line = &self.lines[end.line];
            let start_byte = char_to_byte_offset(start_line, start.column);
            let end_byte = char_to_byte_offset(end_line, end.column);

            let merged = format!("{}{}", &start_line[..start_byte], &end_line[end_byte..]);
            self.lines
                .splice(start.line..=end.line, std::iter::once(merged));
        }

        self.version += 1;
    }

    /// Extract the text within the given range.
    pub fn text_in_range(&self, range: TextRange) -> String {
        let range = range.ordered();
        let start = self.clamp_position(range.start);
        let end = self.clamp_position(range.end);

        if start == end {
            return String::new();
        }

        if start.line == end.line {
            let line = &self.lines[start.line];
            let start_byte = char_to_byte_offset(line, start.column);
            let end_byte = char_to_byte_offset(line, end.column);
            return line[start_byte..end_byte].to_string();
        }

        let mut result = String::new();
        let first = &self.lines[start.line];
        let start_byte = char_to_byte_offset(first, start.column);
        result.push_str(&first[start_byte..]);

        for line_idx in (start.line + 1)..end.line {
            result.push('\n');
            result.push_str(&self.lines[line_idx]);
        }

        result.push('\n');
        let last = &self.lines[end.line];
        let end_byte = char_to_byte_offset(last, end.column);
        result.push_str(&last[..end_byte]);

        result
    }
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self::new()
    }
}

fn char_to_byte_offset(s: &str, char_offset: usize) -> usize {
    s.char_indices()
        .nth(char_offset)
        .map(|(idx, _)| idx)
        .unwrap_or(s.len())
}

/// A cursor within a text buffer, optionally with a selection and preferred column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    /// The current position of this cursor.
    pub position: TextPosition,
    /// An optional selection range anchored from this cursor.
    pub selection: Option<TextRange>,
    /// The preferred column for vertical movement (remembers column across short lines).
    pub preferred_column: Option<usize>,
}

impl Cursor {
    /// Create a new cursor at the given position with no selection.
    pub fn new(position: TextPosition) -> Self {
        Self {
            position,
            selection: None,
            preferred_column: None,
        }
    }
}

/// Multiple cursors for simultaneous editing at several positions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiCursor {
    cursors: Vec<Cursor>,
}

impl MultiCursor {
    /// Create a new multi-cursor set with a single cursor at position (0, 0).
    pub fn new() -> Self {
        Self {
            cursors: vec![Cursor::new(TextPosition::zero())],
        }
    }

    /// Add a cursor to the set.
    pub fn add_cursor(&mut self, cursor: Cursor) {
        self.cursors.push(cursor);
    }

    /// Return a reference to the primary (first) cursor.
    pub fn primary(&self) -> Option<&Cursor> {
        self.cursors.first()
    }

    /// Return a mutable reference to the primary (first) cursor.
    pub fn primary_mut(&mut self) -> Option<&mut Cursor> {
        self.cursors.first_mut()
    }

    /// Return a slice of all cursors.
    pub fn cursors(&self) -> &[Cursor] {
        &self.cursors
    }

    /// Return the number of cursors.
    pub fn len(&self) -> usize {
        self.cursors.len()
    }

    /// Whether the cursor set is empty.
    pub fn is_empty(&self) -> bool {
        self.cursors.is_empty()
    }

    /// Clear all cursors except the primary.
    pub fn clear(&mut self) {
        self.cursors.truncate(1);
    }

    /// Remove and return the cursor at the given index.
    ///
    /// Returns `None` if the index is out of bounds or if removing it would
    /// leave the set empty.
    pub fn remove_cursor(&mut self, index: usize) -> Option<Cursor> {
        if index >= self.cursors.len() || self.cursors.len() <= 1 {
            return None;
        }
        Some(self.cursors.remove(index))
    }
}

impl Default for MultiCursor {
    fn default() -> Self {
        Self::new()
    }
}

/// A single edit operation that can be undone or redone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EditOperation {
    /// Text was inserted at a position.
    Insert {
        /// The position where text was inserted.
        position: TextPosition,
        /// The text that was inserted.
        text: String,
    },
    /// Text was deleted from a range.
    Delete {
        /// The range that was deleted.
        range: TextRange,
        /// The text that was removed.
        deleted_text: String,
    },
}

/// Undo/redo history that groups edit operations.
///
/// Operations are recorded into a current group, which is finalized via
/// `finish_group`. Undo pops the last group and returns its operations;
/// redo re-applies a previously undone group.
#[derive(Debug, Clone)]
pub struct UndoHistory {
    undo_stack: Vec<Vec<EditOperation>>,
    redo_stack: Vec<Vec<EditOperation>>,
    current_group: Vec<EditOperation>,
    max_history: usize,
}

impl UndoHistory {
    /// Create a new undo history with the given maximum number of groups.
    pub fn new(max_history: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            current_group: Vec::new(),
            max_history: max_history.max(1),
        }
    }

    /// Record an edit operation into the current group.
    ///
    /// Recording clears the redo stack (branching history).
    pub fn record(&mut self, op: EditOperation) {
        self.current_group.push(op);
        self.redo_stack.clear();
    }

    /// Finalize the current group and push it onto the undo stack.
    ///
    /// Does nothing if the current group is empty.
    pub fn finish_group(&mut self) {
        if self.current_group.is_empty() {
            return;
        }
        let group = std::mem::take(&mut self.current_group);
        self.undo_stack.push(group);
        if self.undo_stack.len() > self.max_history {
            self.undo_stack.remove(0);
        }
    }

    /// Pop the most recent undo group and return its operations.
    ///
    /// The group is moved to the redo stack. Any pending current group is
    /// finalized first.
    pub fn undo(&mut self) -> Option<Vec<EditOperation>> {
        self.finish_group();
        let group = self.undo_stack.pop()?;
        self.redo_stack.push(group.clone());
        Some(group)
    }

    /// Pop the most recent redo group and return its operations.
    ///
    /// The group is moved back to the undo stack.
    pub fn redo(&mut self) -> Option<Vec<EditOperation>> {
        let group = self.redo_stack.pop()?;
        self.undo_stack.push(group.clone());
        Some(group)
    }

    /// Whether there are groups available to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty() || !self.current_group.is_empty()
    }

    /// Whether there are groups available to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Clear all undo and redo history.
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.current_group.clear();
    }
}

/// A single find result indicating where a match was found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindResult {
    /// The range of the matched text.
    pub range: TextRange,
    /// The line number of the match.
    pub line: usize,
}

/// Find and replace engine for text buffers.
#[derive(Debug, Clone)]
pub struct FindReplace {
    query: String,
    case_sensitive: bool,
    results: Vec<FindResult>,
    current_match: Option<usize>,
}

impl FindReplace {
    /// Create a new empty find/replace state.
    pub fn new() -> Self {
        Self {
            query: String::new(),
            case_sensitive: false,
            results: Vec::new(),
            current_match: None,
        }
    }

    /// Set the search query and case sensitivity. Clears previous results.
    pub fn set_query(&mut self, query: String, case_sensitive: bool) {
        self.query = query;
        self.case_sensitive = case_sensitive;
        self.results.clear();
        self.current_match = None;
    }

    /// Search the buffer for all occurrences of the query.
    pub fn search(&mut self, buffer: &TextBuffer) {
        self.results.clear();
        self.current_match = None;

        if self.query.is_empty() {
            return;
        }

        let query = if self.case_sensitive {
            self.query.clone()
        } else {
            self.query.to_lowercase()
        };
        let query_char_len = query.chars().count();

        for (line_idx, line) in buffer.lines.iter().enumerate() {
            let haystack = if self.case_sensitive {
                line.clone()
            } else {
                line.to_lowercase()
            };

            let mut search_start = 0;
            while let Some(byte_pos) = haystack[search_start..].find(&query) {
                let abs_byte_pos = search_start + byte_pos;
                let char_offset = line[..abs_byte_pos].chars().count();
                let start = TextPosition::new(line_idx, char_offset);
                let end = TextPosition::new(line_idx, char_offset + query_char_len);

                self.results.push(FindResult {
                    range: TextRange::new(start, end),
                    line: line_idx,
                });

                search_start = abs_byte_pos + query.len().max(1);
                if search_start >= haystack.len() {
                    break;
                }
            }
        }

        if !self.results.is_empty() {
            self.current_match = Some(0);
        }
    }

    /// Return all find results.
    pub fn results(&self) -> &[FindResult] {
        &self.results
    }

    /// Return the currently highlighted match.
    pub fn current_match(&self) -> Option<&FindResult> {
        self.current_match.and_then(|idx| self.results.get(idx))
    }

    /// Advance to the next match and return it, wrapping around.
    pub fn next_match(&mut self) -> Option<&FindResult> {
        if self.results.is_empty() {
            return None;
        }
        let next = match self.current_match {
            Some(idx) => (idx + 1) % self.results.len(),
            None => 0,
        };
        self.current_match = Some(next);
        self.results.get(next)
    }

    /// Move to the previous match and return it, wrapping around.
    pub fn prev_match(&mut self) -> Option<&FindResult> {
        if self.results.is_empty() {
            return None;
        }
        let prev = match self.current_match {
            Some(0) => self.results.len() - 1,
            Some(idx) => idx - 1,
            None => self.results.len() - 1,
        };
        self.current_match = Some(prev);
        self.results.get(prev)
    }

    /// Return the total number of matches.
    pub fn match_count(&self) -> usize {
        self.results.len()
    }
}

impl Default for FindReplace {
    fn default() -> Self {
        Self::new()
    }
}

/// A collapsible code fold region spanning a range of lines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoldRegion {
    /// The first line of the fold region (inclusive).
    pub start_line: usize,
    /// The last line of the fold region (inclusive).
    pub end_line: usize,
    /// Whether the region is currently collapsed.
    pub collapsed: bool,
}

/// Tracks code fold regions and their collapsed state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FoldState {
    regions: Vec<FoldRegion>,
}

impl FoldState {
    /// Create a new empty fold state.
    pub fn new() -> Self {
        Self {
            regions: Vec::new(),
        }
    }

    /// Add a new fold region (initially expanded).
    ///
    /// The region is ignored if `start >= end`.
    pub fn add_region(&mut self, start: usize, end: usize) {
        if start >= end {
            return;
        }
        self.regions.push(FoldRegion {
            start_line: start,
            end_line: end,
            collapsed: false,
        });
    }

    /// Toggle the collapsed state of the region at the given index.
    pub fn toggle(&mut self, index: usize) {
        if let Some(region) = self.regions.get_mut(index) {
            region.collapsed = !region.collapsed;
        }
    }

    /// Whether a given line is visible (not hidden by a collapsed fold).
    pub fn is_line_visible(&self, line: usize) -> bool {
        for region in &self.regions {
            if region.collapsed && line > region.start_line && line <= region.end_line {
                return false;
            }
        }
        true
    }

    /// Count how many lines are visible given the total number of lines.
    pub fn visible_line_count(&self, total_lines: usize) -> usize {
        (0..total_lines)
            .filter(|line| self.is_line_visible(*line))
            .count()
    }

    /// Return a slice of all fold regions.
    pub fn regions(&self) -> &[FoldRegion] {
        &self.regions
    }
}

/// The severity level of an inline diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    /// A compiler or runtime error.
    Error,
    /// A warning that may indicate a problem.
    Warning,
    /// Informational note.
    Info,
    /// A hint or suggestion.
    Hint,
}

/// An inline diagnostic attached to a text range with severity and message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineDiagnostic {
    /// The text range this diagnostic applies to.
    pub range: TextRange,
    /// The severity of the diagnostic.
    pub severity: DiagnosticSeverity,
    /// Human-readable diagnostic message.
    pub message: String,
    /// The source tool or linter that produced this diagnostic.
    pub source: Option<String>,
}

/// A collection of diagnostics for a buffer.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticSet {
    diagnostics: Vec<InlineDiagnostic>,
}

impl DiagnosticSet {
    /// Create a new empty diagnostic set.
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }

    /// Replace all diagnostics with the given list.
    pub fn set(&mut self, diagnostics: Vec<InlineDiagnostic>) {
        self.diagnostics = diagnostics;
    }

    /// Return all diagnostics whose range intersects the given line.
    pub fn for_line(&self, line: usize) -> Vec<&InlineDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.range.start.line <= line && d.range.end.line >= line)
            .collect()
    }

    /// Return all diagnostics with the given severity.
    pub fn by_severity(&self, severity: DiagnosticSeverity) -> Vec<&InlineDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == severity)
            .collect()
    }

    /// Return the total number of diagnostics.
    pub fn count(&self) -> usize {
        self.diagnostics.len()
    }

    /// Clear all diagnostics.
    pub fn clear(&mut self) {
        self.diagnostics.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod text_buffer_tests {
        use super::*;

        #[test]
        fn new_buffer_has_one_empty_line() {
            let buf = TextBuffer::new();
            assert_eq!(buf.line_count(), 1);
            assert_eq!(buf.line(0), Some(""));
            assert_eq!(buf.text(), "");
            assert!(buf.is_empty());
            assert_eq!(buf.version(), 0);
        }

        #[test]
        fn from_str_single_line() {
            let buf = TextBuffer::from_str("hello world");
            assert_eq!(buf.line_count(), 1);
            assert_eq!(buf.line(0), Some("hello world"));
            assert_eq!(buf.text(), "hello world");
        }

        #[test]
        fn from_str_multi_line() {
            let buf = TextBuffer::from_str("line1\nline2\nline3");
            assert_eq!(buf.line_count(), 3);
            assert_eq!(buf.line(0), Some("line1"));
            assert_eq!(buf.line(1), Some("line2"));
            assert_eq!(buf.line(2), Some("line3"));
        }

        #[test]
        fn from_str_empty() {
            let buf = TextBuffer::from_str("");
            assert_eq!(buf.line_count(), 1);
            assert!(buf.is_empty());
        }

        #[test]
        fn len_chars_counts_newlines() {
            let buf = TextBuffer::from_str("ab\ncd\ne");
            assert_eq!(buf.len_chars(), 7);
        }

        #[test]
        fn line_len_returns_char_count() {
            let buf = TextBuffer::from_str("hello\nworld!");
            assert_eq!(buf.line_len(0), Some(5));
            assert_eq!(buf.line_len(1), Some(6));
            assert_eq!(buf.line_len(2), None);
        }

        #[test]
        fn insert_within_single_line() {
            let mut buf = TextBuffer::from_str("helo");
            buf.insert(TextPosition::new(0, 2), "l");
            assert_eq!(buf.text(), "hello");
            assert_eq!(buf.version(), 1);
        }

        #[test]
        fn insert_at_beginning() {
            let mut buf = TextBuffer::from_str("world");
            buf.insert(TextPosition::new(0, 0), "hello ");
            assert_eq!(buf.text(), "hello world");
        }

        #[test]
        fn insert_at_end() {
            let mut buf = TextBuffer::from_str("hello");
            buf.insert(TextPosition::new(0, 5), " world");
            assert_eq!(buf.text(), "hello world");
        }

        #[test]
        fn insert_newline_splits_line() {
            let mut buf = TextBuffer::from_str("helloworld");
            buf.insert(TextPosition::new(0, 5), "\n");
            assert_eq!(buf.line_count(), 2);
            assert_eq!(buf.line(0), Some("hello"));
            assert_eq!(buf.line(1), Some("world"));
        }

        #[test]
        fn insert_multi_line_text() {
            let mut buf = TextBuffer::from_str("start end");
            buf.insert(TextPosition::new(0, 6), "mid1\nmid2\n");
            assert_eq!(buf.text(), "start mid1\nmid2\nend");
            assert_eq!(buf.line_count(), 3);
        }

        #[test]
        fn insert_empty_text_no_change() {
            let mut buf = TextBuffer::from_str("hello");
            buf.insert(TextPosition::new(0, 2), "");
            assert_eq!(buf.text(), "hello");
            assert_eq!(buf.version(), 0);
        }

        #[test]
        fn delete_within_single_line() {
            let mut buf = TextBuffer::from_str("hello world");
            buf.delete(TextRange::new(
                TextPosition::new(0, 5),
                TextPosition::new(0, 11),
            ));
            assert_eq!(buf.text(), "hello");
            assert_eq!(buf.version(), 1);
        }

        #[test]
        fn delete_across_lines() {
            let mut buf = TextBuffer::from_str("hello\nworld");
            buf.delete(TextRange::new(
                TextPosition::new(0, 3),
                TextPosition::new(1, 2),
            ));
            assert_eq!(buf.text(), "helrld");
            assert_eq!(buf.line_count(), 1);
        }

        #[test]
        fn delete_entire_line_merge() {
            let mut buf = TextBuffer::from_str("line1\nline2\nline3");
            buf.delete(TextRange::new(
                TextPosition::new(0, 5),
                TextPosition::new(1, 5),
            ));
            assert_eq!(buf.text(), "line1\nline3");
            assert_eq!(buf.line_count(), 2);
        }

        #[test]
        fn delete_empty_range_no_change() {
            let mut buf = TextBuffer::from_str("hello");
            buf.delete(TextRange::new(
                TextPosition::new(0, 2),
                TextPosition::new(0, 2),
            ));
            assert_eq!(buf.text(), "hello");
            assert_eq!(buf.version(), 0);
        }

        #[test]
        fn text_in_range_single_line() {
            let buf = TextBuffer::from_str("hello world");
            let text = buf.text_in_range(TextRange::new(
                TextPosition::new(0, 0),
                TextPosition::new(0, 5),
            ));
            assert_eq!(text, "hello");
        }

        #[test]
        fn text_in_range_multi_line() {
            let buf = TextBuffer::from_str("hello\nworld\nfoo");
            let text = buf.text_in_range(TextRange::new(
                TextPosition::new(0, 3),
                TextPosition::new(2, 2),
            ));
            assert_eq!(text, "lo\nworld\nfo");
        }

        #[test]
        fn insert_and_delete_roundtrip() {
            let mut buf = TextBuffer::from_str("hello world");
            buf.insert(TextPosition::new(0, 5), " beautiful");
            assert_eq!(buf.text(), "hello beautiful world");
            buf.delete(TextRange::new(
                TextPosition::new(0, 5),
                TextPosition::new(0, 15),
            ));
            assert_eq!(buf.text(), "hello world");
        }

        #[test]
        fn insert_with_unicode() {
            let mut buf = TextBuffer::from_str("café");
            assert_eq!(buf.line_len(0), Some(4));
            buf.insert(TextPosition::new(0, 4), " ☕");
            assert_eq!(buf.text(), "café ☕");
            assert_eq!(buf.len_chars(), 6);
        }

        #[test]
        fn clamped_position_on_out_of_bounds() {
            let mut buf = TextBuffer::from_str("hi");
            buf.insert(TextPosition::new(100, 100), "!");
            assert_eq!(buf.text(), "hi!");
        }
    }

    mod cursor_tests {
        use super::*;

        #[test]
        fn multi_cursor_default() {
            let mc = MultiCursor::new();
            assert_eq!(mc.len(), 1);
            assert!(!mc.is_empty());
            assert_eq!(mc.primary().unwrap().position, TextPosition::zero());
        }

        #[test]
        fn add_and_remove_cursors() {
            let mut mc = MultiCursor::new();
            mc.add_cursor(Cursor::new(TextPosition::new(1, 5)));
            mc.add_cursor(Cursor::new(TextPosition::new(2, 0)));
            assert_eq!(mc.len(), 3);

            let removed = mc.remove_cursor(1);
            assert!(removed.is_some());
            assert_eq!(mc.len(), 2);
        }

        #[test]
        fn cannot_remove_last_cursor() {
            let mut mc = MultiCursor::new();
            assert!(mc.remove_cursor(0).is_none());
            assert_eq!(mc.len(), 1);
        }

        #[test]
        fn clear_keeps_primary() {
            let mut mc = MultiCursor::new();
            mc.add_cursor(Cursor::new(TextPosition::new(1, 0)));
            mc.add_cursor(Cursor::new(TextPosition::new(2, 0)));
            mc.clear();
            assert_eq!(mc.len(), 1);
            assert_eq!(mc.primary().unwrap().position, TextPosition::zero());
        }

        #[test]
        fn primary_mut_modifies_first() {
            let mut mc = MultiCursor::new();
            mc.primary_mut().unwrap().position = TextPosition::new(5, 10);
            assert_eq!(mc.primary().unwrap().position, TextPosition::new(5, 10));
        }
    }

    mod undo_history_tests {
        use super::*;

        #[test]
        fn record_and_undo() {
            let mut history = UndoHistory::new(100);
            history.record(EditOperation::Insert {
                position: TextPosition::new(0, 0),
                text: "hello".to_string(),
            });
            history.finish_group();

            assert!(history.can_undo());
            let ops = history.undo().unwrap();
            assert_eq!(ops.len(), 1);
            assert!(!history.can_undo());
        }

        #[test]
        fn undo_then_redo() {
            let mut history = UndoHistory::new(100);
            history.record(EditOperation::Insert {
                position: TextPosition::zero(),
                text: "a".to_string(),
            });
            history.finish_group();

            history.undo();
            assert!(history.can_redo());

            let ops = history.redo().unwrap();
            assert_eq!(ops.len(), 1);
            assert!(!history.can_redo());
        }

        #[test]
        fn recording_clears_redo() {
            let mut history = UndoHistory::new(100);
            history.record(EditOperation::Insert {
                position: TextPosition::zero(),
                text: "a".to_string(),
            });
            history.finish_group();
            history.undo();
            assert!(history.can_redo());

            history.record(EditOperation::Insert {
                position: TextPosition::zero(),
                text: "b".to_string(),
            });
            assert!(!history.can_redo());
        }

        #[test]
        fn max_history_evicts_oldest() {
            let mut history = UndoHistory::new(2);
            for i in 0..5 {
                history.record(EditOperation::Insert {
                    position: TextPosition::zero(),
                    text: format!("{}", i),
                });
                history.finish_group();
            }

            let mut count = 0;
            while history.undo().is_some() {
                count += 1;
            }
            assert_eq!(count, 2);
        }

        #[test]
        fn grouped_operations() {
            let mut history = UndoHistory::new(100);
            history.record(EditOperation::Insert {
                position: TextPosition::zero(),
                text: "a".to_string(),
            });
            history.record(EditOperation::Insert {
                position: TextPosition::new(0, 1),
                text: "b".to_string(),
            });
            history.finish_group();

            let ops = history.undo().unwrap();
            assert_eq!(ops.len(), 2);
        }

        #[test]
        fn finish_empty_group_is_noop() {
            let mut history = UndoHistory::new(100);
            history.finish_group();
            assert!(!history.can_undo());
        }

        #[test]
        fn clear_resets_all() {
            let mut history = UndoHistory::new(100);
            history.record(EditOperation::Insert {
                position: TextPosition::zero(),
                text: "x".to_string(),
            });
            history.finish_group();
            history.clear();
            assert!(!history.can_undo());
            assert!(!history.can_redo());
        }
    }

    mod find_replace_tests {
        use super::*;

        #[test]
        fn search_finds_all_occurrences() {
            let buf = TextBuffer::from_str("foo bar foo baz foo");
            let mut fr = FindReplace::new();
            fr.set_query("foo".to_string(), true);
            fr.search(&buf);
            assert_eq!(fr.match_count(), 3);
        }

        #[test]
        fn search_case_insensitive() {
            let buf = TextBuffer::from_str("Hello HELLO hello");
            let mut fr = FindReplace::new();
            fr.set_query("hello".to_string(), false);
            fr.search(&buf);
            assert_eq!(fr.match_count(), 3);
        }

        #[test]
        fn search_case_sensitive() {
            let buf = TextBuffer::from_str("Hello HELLO hello");
            let mut fr = FindReplace::new();
            fr.set_query("hello".to_string(), true);
            fr.search(&buf);
            assert_eq!(fr.match_count(), 1);
        }

        #[test]
        fn search_multi_line() {
            let buf = TextBuffer::from_str("abc\ndef abc\nghi");
            let mut fr = FindReplace::new();
            fr.set_query("abc".to_string(), true);
            fr.search(&buf);
            assert_eq!(fr.match_count(), 2);
            assert_eq!(fr.results()[0].line, 0);
            assert_eq!(fr.results()[1].line, 1);
        }

        #[test]
        fn next_and_prev_wrap() {
            let buf = TextBuffer::from_str("a a a");
            let mut fr = FindReplace::new();
            fr.set_query("a".to_string(), true);
            fr.search(&buf);
            assert_eq!(fr.match_count(), 3);

            assert_eq!(fr.current_match().unwrap().range.start.column, 0);
            fr.next_match();
            assert_eq!(fr.current_match().unwrap().range.start.column, 2);
            fr.next_match();
            assert_eq!(fr.current_match().unwrap().range.start.column, 4);
            fr.next_match();
            assert_eq!(fr.current_match().unwrap().range.start.column, 0);

            fr.prev_match();
            assert_eq!(fr.current_match().unwrap().range.start.column, 4);
        }

        #[test]
        fn empty_query_no_results() {
            let buf = TextBuffer::from_str("hello");
            let mut fr = FindReplace::new();
            fr.set_query(String::new(), true);
            fr.search(&buf);
            assert_eq!(fr.match_count(), 0);
            assert!(fr.current_match().is_none());
        }

        #[test]
        fn no_matches_returns_none() {
            let buf = TextBuffer::from_str("hello");
            let mut fr = FindReplace::new();
            fr.set_query("xyz".to_string(), true);
            fr.search(&buf);
            assert_eq!(fr.match_count(), 0);
            assert!(fr.next_match().is_none());
            assert!(fr.prev_match().is_none());
        }
    }

    mod fold_state_tests {
        use super::*;

        #[test]
        fn all_visible_by_default() {
            let fold = FoldState::new();
            for i in 0..10 {
                assert!(fold.is_line_visible(i));
            }
            assert_eq!(fold.visible_line_count(10), 10);
        }

        #[test]
        fn collapsed_region_hides_inner_lines() {
            let mut fold = FoldState::new();
            fold.add_region(2, 5);
            fold.toggle(0);

            assert!(fold.is_line_visible(0));
            assert!(fold.is_line_visible(1));
            assert!(fold.is_line_visible(2));
            assert!(!fold.is_line_visible(3));
            assert!(!fold.is_line_visible(4));
            assert!(!fold.is_line_visible(5));
            assert!(fold.is_line_visible(6));
        }

        #[test]
        fn visible_line_count_with_fold() {
            let mut fold = FoldState::new();
            fold.add_region(2, 5);
            fold.toggle(0);
            assert_eq!(fold.visible_line_count(10), 7);
        }

        #[test]
        fn toggle_unfolds() {
            let mut fold = FoldState::new();
            fold.add_region(1, 3);
            fold.toggle(0);
            assert!(!fold.is_line_visible(2));
            fold.toggle(0);
            assert!(fold.is_line_visible(2));
        }

        #[test]
        fn invalid_region_ignored() {
            let mut fold = FoldState::new();
            fold.add_region(5, 5);
            fold.add_region(5, 3);
            assert_eq!(fold.regions().len(), 0);
        }

        #[test]
        fn multiple_collapsed_regions() {
            let mut fold = FoldState::new();
            fold.add_region(1, 3);
            fold.add_region(6, 8);
            fold.toggle(0);
            fold.toggle(1);
            assert_eq!(fold.visible_line_count(10), 6);
        }
    }

    mod diagnostic_tests {
        use super::*;

        #[test]
        fn empty_diagnostics() {
            let ds = DiagnosticSet::new();
            assert_eq!(ds.count(), 0);
            assert!(ds.for_line(0).is_empty());
        }

        #[test]
        fn set_and_query() {
            let mut ds = DiagnosticSet::new();
            ds.set(vec![
                InlineDiagnostic {
                    range: TextRange::new(TextPosition::new(0, 0), TextPosition::new(0, 5)),
                    severity: DiagnosticSeverity::Error,
                    message: "undefined variable".to_string(),
                    source: Some("rustc".to_string()),
                },
                InlineDiagnostic {
                    range: TextRange::new(TextPosition::new(2, 0), TextPosition::new(2, 3)),
                    severity: DiagnosticSeverity::Warning,
                    message: "unused import".to_string(),
                    source: None,
                },
                InlineDiagnostic {
                    range: TextRange::new(TextPosition::new(0, 10), TextPosition::new(0, 15)),
                    severity: DiagnosticSeverity::Error,
                    message: "type mismatch".to_string(),
                    source: Some("rustc".to_string()),
                },
            ]);

            assert_eq!(ds.count(), 3);
            assert_eq!(ds.for_line(0).len(), 2);
            assert_eq!(ds.for_line(1).len(), 0);
            assert_eq!(ds.for_line(2).len(), 1);
            assert_eq!(ds.by_severity(DiagnosticSeverity::Error).len(), 2);
            assert_eq!(ds.by_severity(DiagnosticSeverity::Warning).len(), 1);
            assert_eq!(ds.by_severity(DiagnosticSeverity::Info).len(), 0);
        }

        #[test]
        fn clear_removes_all() {
            let mut ds = DiagnosticSet::new();
            ds.set(vec![InlineDiagnostic {
                range: TextRange::new(TextPosition::new(0, 0), TextPosition::new(0, 1)),
                severity: DiagnosticSeverity::Hint,
                message: "hint".to_string(),
                source: None,
            }]);
            assert_eq!(ds.count(), 1);
            ds.clear();
            assert_eq!(ds.count(), 0);
        }

        #[test]
        fn multi_line_diagnostic() {
            let mut ds = DiagnosticSet::new();
            ds.set(vec![InlineDiagnostic {
                range: TextRange::new(TextPosition::new(1, 0), TextPosition::new(3, 5)),
                severity: DiagnosticSeverity::Error,
                message: "spans multiple lines".to_string(),
                source: None,
            }]);
            assert_eq!(ds.for_line(0).len(), 0);
            assert_eq!(ds.for_line(1).len(), 1);
            assert_eq!(ds.for_line(2).len(), 1);
            assert_eq!(ds.for_line(3).len(), 1);
            assert_eq!(ds.for_line(4).len(), 0);
        }
    }

    mod text_range_tests {
        use super::*;

        #[test]
        fn empty_range() {
            let range = TextRange::new(TextPosition::new(1, 5), TextPosition::new(1, 5));
            assert!(range.is_empty());
        }

        #[test]
        fn ordered_swaps_if_needed() {
            let range = TextRange::new(TextPosition::new(2, 0), TextPosition::new(0, 5));
            let ordered = range.ordered();
            assert_eq!(ordered.start, TextPosition::new(0, 5));
            assert_eq!(ordered.end, TextPosition::new(2, 0));
        }
    }
}

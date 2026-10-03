use crate::components::scrollable::scrollable_vertical;
use crate::icon_config::resolve_icon_path;
use crate::theme::{Theme, use_theme};
use kael::{prelude::FluentBuilder as _, *};
use regex::Regex;
use ropey::Rope;
use std::cmp::min;
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use unicode_segmentation::UnicodeSegmentation;

// Tree-sitter's native C runtime does not target `wasm32-unknown-unknown`.
// Keep the editor's public surface and all in-memory editing behavior available
// in browsers, while making syntax parsing an explicit no-op there.  The real
// dependency and implementation remain unchanged on native targets.
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
#[allow(clippy::result_unit_err, clippy::unnecessary_wraps)]
pub mod tree_sitter {
    use std::ops::Range;

    #[derive(Clone, Copy, Debug, Default)]
    pub struct Language;

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Point {
        pub row: usize,
        pub column: usize,
    }

    impl Point {
        pub const fn new(row: usize, column: usize) -> Self {
            Self { row, column }
        }
    }

    #[derive(Clone, Copy, Debug, Default)]
    pub struct InputEdit {
        pub start_byte: usize,
        pub old_end_byte: usize,
        pub new_end_byte: usize,
        pub start_position: Point,
        pub old_end_position: Point,
        pub new_end_position: Point,
    }

    #[derive(Clone, Debug, Default)]
    pub struct Tree;

    impl Tree {
        pub fn edit(&mut self, _edit: &InputEdit) {}

        pub fn root_node(&self) -> Node {
            Node
        }
    }

    #[derive(Clone, Copy, Debug, Default)]
    pub struct Node;

    impl Node {
        pub fn walk(self) -> TreeCursor {
            TreeCursor
        }

        pub fn kind(self) -> &'static str {
            ""
        }

        pub fn start_position(self) -> Point {
            Point::default()
        }

        pub fn end_position(self) -> Point {
            Point::default()
        }

        pub fn descendant_for_point_range(self, _start: Point, _end: Point) -> Option<Self> {
            None
        }

        pub fn parent(self) -> Option<Self> {
            None
        }

        pub fn child_count(self) -> usize {
            0
        }

        pub fn child(self, _index: usize) -> Option<Self> {
            None
        }

        pub fn byte_range(self) -> Range<usize> {
            0..0
        }

        pub fn start_byte(self) -> usize {
            0
        }

        pub fn end_byte(self) -> usize {
            0
        }
    }

    #[derive(Clone, Copy, Debug, Default)]
    pub struct TreeCursor;

    impl TreeCursor {
        pub fn node(self) -> Node {
            Node
        }

        pub fn goto_first_child(&mut self) -> bool {
            false
        }

        pub fn goto_next_sibling(&mut self) -> bool {
            false
        }

        pub fn goto_parent(&mut self) -> bool {
            false
        }
    }

    #[derive(Debug, Default)]
    pub struct Parser;

    impl Parser {
        pub fn new() -> Self {
            Self
        }

        pub fn set_language(&mut self, _language: &Language) -> Result<(), ()> {
            Ok(())
        }

        pub fn parse(&mut self, _content: &str, _old_tree: Option<&Tree>) -> Option<Tree> {
            None
        }

        pub fn parse_with_options<T, F>(
            &mut self,
            _callback: &mut F,
            _old_tree: Option<&Tree>,
            _options: Option<()>,
        ) -> Option<Tree>
        where
            T: AsRef<[u8]>,
            F: FnMut(usize, Point) -> T,
        {
            None
        }
    }

    #[derive(Debug, Default)]
    pub struct Query {
        capture_names: Vec<&'static str>,
    }

    impl Query {
        pub fn new(_language: &Language, _source: &str) -> Result<Self, ()> {
            Ok(Self::default())
        }

        pub fn capture_names(&self) -> &[&'static str] {
            &self.capture_names
        }
    }

    #[derive(Clone, Copy, Debug, Default)]
    pub struct QueryCapture {
        pub index: u32,
        pub node: Node,
    }

    #[derive(Debug, Default)]
    pub struct QueryMatch {
        pub captures: &'static [QueryCapture],
    }

    #[derive(Debug, Default)]
    pub struct QueryMatches;

    pub trait StreamingIterator {
        type Item;

        fn next(&mut self) -> Option<&Self::Item>;
    }

    impl StreamingIterator for QueryMatches {
        type Item = QueryMatch;

        fn next(&mut self) -> Option<&Self::Item> {
            None
        }
    }

    #[derive(Debug, Default)]
    pub struct QueryCursor;

    impl QueryCursor {
        pub fn new() -> Self {
            Self
        }

        pub fn set_byte_range(&mut self, _range: Range<usize>) {}

        pub fn matches<F, T>(
            &mut self,
            _query: &Query,
            _node: Node,
            _text_provider: F,
        ) -> QueryMatches
        where
            F: FnMut(Node) -> T,
        {
            QueryMatches
        }
    }
}

use tree_sitter::{
    InputEdit, Parser, Point as TSPoint, Query, QueryCursor, StreamingIterator, Tree,
};

actions!(
    editor,
    [
        MoveUp,
        MoveDown,
        MoveLeft,
        MoveRight,
        MoveToLineStart,
        MoveToLineEnd,
        MoveToDocStart,
        MoveToDocEnd,
        MoveWordLeft,
        MoveWordRight,
        PageUp,
        PageDown,
        SelectUp,
        SelectDown,
        SelectLeft,
        SelectRight,
        SelectToLineStart,
        SelectToLineEnd,
        SelectAll,
        Backspace,
        Delete,
        DeleteWord,
        Enter,
        Tab,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
    ]
);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", MoveUp, Some("Editor")),
        KeyBinding::new("down", MoveDown, Some("Editor")),
        KeyBinding::new("left", MoveLeft, Some("Editor")),
        KeyBinding::new("right", MoveRight, Some("Editor")),
        KeyBinding::new("home", MoveToLineStart, Some("Editor")),
        KeyBinding::new("end", MoveToLineEnd, Some("Editor")),
        #[cfg(target_os = "macos")]
        KeyBinding::new("alt-left", MoveWordLeft, Some("Editor")),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-left", MoveWordLeft, Some("Editor")),
        #[cfg(target_os = "macos")]
        KeyBinding::new("alt-right", MoveWordRight, Some("Editor")),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-right", MoveWordRight, Some("Editor")),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-up", MoveToDocStart, Some("Editor")),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-home", MoveToDocStart, Some("Editor")),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-down", MoveToDocEnd, Some("Editor")),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-end", MoveToDocEnd, Some("Editor")),
        KeyBinding::new("pageup", PageUp, Some("Editor")),
        KeyBinding::new("pagedown", PageDown, Some("Editor")),
        KeyBinding::new("shift-up", SelectUp, Some("Editor")),
        KeyBinding::new("shift-down", SelectDown, Some("Editor")),
        KeyBinding::new("shift-left", SelectLeft, Some("Editor")),
        KeyBinding::new("shift-right", SelectRight, Some("Editor")),
        KeyBinding::new("shift-home", SelectToLineStart, Some("Editor")),
        KeyBinding::new("shift-end", SelectToLineEnd, Some("Editor")),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-a", SelectAll, Some("Editor")),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-a", SelectAll, Some("Editor")),
        KeyBinding::new("backspace", Backspace, Some("Editor")),
        KeyBinding::new("delete", Delete, Some("Editor")),
        #[cfg(target_os = "macos")]
        KeyBinding::new("alt-backspace", DeleteWord, Some("Editor")),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-backspace", DeleteWord, Some("Editor")),
        KeyBinding::new("enter", Enter, Some("Editor")),
        KeyBinding::new("tab", Tab, Some("Editor")),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-c", Copy, Some("Editor")),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-c", Copy, Some("Editor")),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-x", Cut, Some("Editor")),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-x", Cut, Some("Editor")),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-v", Paste, Some("Editor")),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-v", Paste, Some("Editor")),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-z", Undo, Some("Editor")),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-z", Undo, Some("Editor")),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-z", Redo, Some("Editor")),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-z", Redo, Some("Editor")),
    ]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub line: usize,
    /// UTF-8 byte column in the line.
    pub col: usize,
}

impl Position {
    pub fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }

    pub fn zero() -> Self {
        Self { line: 0, col: 0 }
    }

    /// Content-safe cursor position summary for diagnostics and agent tools.
    pub fn to_text(&self) -> String {
        format!("position(line={}, col={})", self.line, self.col)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Position,
    pub cursor: Position,
}

impl Selection {
    pub fn new(anchor: Position, cursor: Position) -> Self {
        Self { anchor, cursor }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.cursor
    }

    pub fn range(&self) -> (Position, Position) {
        if self.anchor <= self.cursor {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        }
    }

    /// Content-safe selection summary that exposes geometry, not selected text.
    pub fn to_text(&self) -> String {
        let (start, end) = self.range();
        format!(
            "selection(empty={}, reversed={}, start_line={}, start_col={}, end_line={}, end_col={}, line_span={})",
            self.is_empty(),
            self.anchor > self.cursor,
            start.line,
            start.col,
            end.line,
            end.col,
            end.line.saturating_sub(start.line) + 1
        )
    }
}

#[derive(Debug, Clone)]
enum EditOp {
    Insert {
        byte_offset: usize,
        text: String,
    },
    Delete {
        byte_offset: usize,
        text: String,
    },
    Replace {
        byte_offset: usize,
        before: String,
        after: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoldRange {
    pub start_line: usize,
    pub end_line: usize,
}

impl FoldRange {
    /// Content-safe fold geometry summary.
    pub fn to_text(&self) -> String {
        format!(
            "fold_range(start_line={}, end_line={}, line_span={})",
            self.start_line,
            self.end_line,
            self.end_line.saturating_sub(self.start_line) + 1
        )
    }
}

const AUTO_CLOSE_PAIRS: &[(char, char)] = &[
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('"', '"'),
    ('\'', '\''),
    ('`', '`'),
];
const MAX_ACCESSIBILITY_VALUE_CHARS: usize = 65_536;
const INLINE_ACCESSIBILITY_DOCUMENT_BYTES: usize = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    JavaScript,
    TypeScript,
    Python,
    Json,
    Toml,
    Markdown,
    Go,
    C,
    Cpp,
    Java,
    Ruby,
    Bash,
    Css,
    Html,
    Yaml,
    Lua,
    Zig,
    Scala,
    Php,
    OCaml,
    Sql,
    Plain,
}

/// Syntax-processing backend available to the editor on the current target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorSyntaxBackend {
    /// Native Tree-sitter parsing, highlighting, folding, and scope discovery.
    TreeSitter,
    /// Browser-safe plain-text editing without Tree-sitter-derived features.
    PlainText,
}

impl EditorSyntaxBackend {
    /// Returns the backend selected by the compilation target.
    pub const fn current() -> Self {
        if cfg!(target_arch = "wasm32") {
            Self::PlainText
        } else {
            Self::TreeSitter
        }
    }

    /// Stable backend key for diagnostics and capability-aware UI.
    pub const fn to_text(self) -> &'static str {
        match self {
            Self::TreeSitter => "tree-sitter",
            Self::PlainText => "plain-text",
        }
    }

    /// Whether syntax trees and Tree-sitter-derived features are available.
    pub const fn supports_syntax_trees(self) -> bool {
        matches!(self, Self::TreeSitter)
    }
}

impl Language {
    /// Stable language key for content-safe diagnostics.
    pub fn to_text(&self) -> &'static str {
        match self {
            Language::Rust => "rust",
            Language::JavaScript => "javascript",
            Language::TypeScript => "typescript",
            Language::Python => "python",
            Language::Json => "json",
            Language::Toml => "toml",
            Language::Markdown => "markdown",
            Language::Go => "go",
            Language::C => "c",
            Language::Cpp => "cpp",
            Language::Java => "java",
            Language::Ruby => "ruby",
            Language::Bash => "bash",
            Language::Css => "css",
            Language::Html => "html",
            Language::Yaml => "yaml",
            Language::Lua => "lua",
            Language::Zig => "zig",
            Language::Scala => "scala",
            Language::Php => "php",
            Language::OCaml => "ocaml",
            Language::Sql => "sql",
            Language::Plain => "plain",
        }
    }

    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "rs" => Language::Rust,
            "js" | "jsx" | "mjs" | "cjs" => Language::JavaScript,
            "ts" | "tsx" => Language::TypeScript,
            "py" | "pyi" => Language::Python,
            "json" | "jsonc" => Language::Json,
            "toml" => Language::Toml,
            "md" | "markdown" => Language::Markdown,
            "go" => Language::Go,
            "c" | "h" => Language::C,
            "cpp" | "cxx" | "cc" | "hpp" | "hxx" | "hh" => Language::Cpp,
            "java" => Language::Java,
            "rb" | "rake" | "gemspec" => Language::Ruby,
            "sh" | "bash" | "zsh" => Language::Bash,
            "css" => Language::Css,
            "html" | "htm" => Language::Html,
            "yml" | "yaml" => Language::Yaml,
            "lua" => Language::Lua,
            "zig" => Language::Zig,
            "scala" | "sc" => Language::Scala,
            "php" => Language::Php,
            "ml" | "mli" => Language::OCaml,
            "sql" => Language::Sql,
            _ => Language::Plain,
        }
    }

    pub fn from_path(path: &std::path::Path) -> Self {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(Self::from_extension)
            .unwrap_or(Language::Plain)
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Language::Rust => "Rust",
            Language::JavaScript => "JavaScript",
            Language::TypeScript => "TypeScript",
            Language::Python => "Python",
            Language::Json => "JSON",
            Language::Toml => "TOML",
            Language::Markdown => "Markdown",
            Language::Go => "Go",
            Language::C => "C",
            Language::Cpp => "C++",
            Language::Java => "Java",
            Language::Ruby => "Ruby",
            Language::Bash => "Shell",
            Language::Css => "CSS",
            Language::Html => "HTML",
            Language::Yaml => "YAML",
            Language::Lua => "Lua",
            Language::Zig => "Zig",
            Language::Scala => "Scala",
            Language::Php => "PHP",
            Language::OCaml => "OCaml",
            Language::Sql => "SQL",
            Language::Plain => "Plain Text",
        }
    }

    /// Returns the native Tree-sitter grammar for this language when available.
    ///
    /// Browser editors always return `None`: the language packages embed C
    /// parsers that do not target `wasm32-unknown-unknown`, so browser builds
    /// intentionally retain the editor's plain-text backend instead.
    pub fn tree_sitter_language(&self) -> Option<tree_sitter::Language> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            match self {
                #[cfg(feature = "tree-sitter-rust")]
                Language::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
                #[cfg(feature = "tree-sitter-javascript")]
                Language::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
                #[cfg(feature = "tree-sitter-typescript")]
                Language::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
                #[cfg(all(
                    feature = "tree-sitter-javascript",
                    not(feature = "tree-sitter-typescript")
                ))]
                Language::TypeScript => Some(tree_sitter_javascript::LANGUAGE.into()),
                #[cfg(feature = "tree-sitter-python")]
                Language::Python => Some(tree_sitter_python::LANGUAGE.into()),
                #[cfg(feature = "tree-sitter-json")]
                Language::Json => Some(tree_sitter_json::LANGUAGE.into()),
                #[cfg(feature = "tree-sitter-toml-ng")]
                Language::Toml => Some(tree_sitter_toml_ng::language()),
                #[cfg(feature = "tree-sitter-md")]
                Language::Markdown => Some(tree_sitter_md::LANGUAGE.into()),
                #[cfg(feature = "tree-sitter-go")]
                Language::Go => Some(tree_sitter_go::LANGUAGE.into()),
                #[cfg(feature = "tree-sitter-c")]
                Language::C => Some(tree_sitter_c::LANGUAGE.into()),
                #[cfg(feature = "tree-sitter-cpp")]
                Language::Cpp => Some(tree_sitter_cpp::LANGUAGE.into()),
                #[cfg(feature = "tree-sitter-java")]
                Language::Java => Some(tree_sitter_java::LANGUAGE.into()),
                #[cfg(feature = "tree-sitter-ruby")]
                Language::Ruby => Some(tree_sitter_ruby::LANGUAGE.into()),
                #[cfg(feature = "tree-sitter-bash")]
                Language::Bash => Some(tree_sitter_bash::LANGUAGE.into()),
                #[cfg(feature = "tree-sitter-css")]
                Language::Css => Some(tree_sitter_css::LANGUAGE.into()),
                #[cfg(feature = "tree-sitter-html")]
                Language::Html => Some(tree_sitter_html::LANGUAGE.into()),
                #[cfg(feature = "tree-sitter-yaml")]
                Language::Yaml => Some(tree_sitter_yaml::LANGUAGE.into()),
                #[cfg(feature = "tree-sitter-lua")]
                Language::Lua => Some(tree_sitter_lua::LANGUAGE.into()),
                #[cfg(feature = "tree-sitter-zig")]
                Language::Zig => Some(tree_sitter_zig::LANGUAGE.into()),
                #[cfg(feature = "tree-sitter-scala")]
                Language::Scala => Some(tree_sitter_scala::LANGUAGE.into()),
                #[cfg(feature = "tree-sitter-php")]
                Language::Php => Some(tree_sitter_php::LANGUAGE_PHP.into()),
                #[cfg(feature = "tree-sitter-ocaml")]
                Language::OCaml => Some(tree_sitter_ocaml::LANGUAGE_OCAML.into()),
                #[cfg(feature = "tree-sitter-sequel")]
                Language::Sql => Some(tree_sitter_sequel::LANGUAGE.into()),
                _ => None,
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            None
        }
    }

    /// Returns the native Tree-sitter highlight query when available.
    ///
    /// Browser editors always return `None` together with
    /// [`Self::tree_sitter_language`].
    pub fn highlight_query_source(&self) -> Option<std::borrow::Cow<'static, str>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            match self {
                #[cfg(feature = "tree-sitter-rust")]
                Language::Rust => Some(tree_sitter_rust::HIGHLIGHTS_QUERY.into()),
                #[cfg(feature = "tree-sitter-javascript")]
                Language::JavaScript => Some(tree_sitter_javascript::HIGHLIGHT_QUERY.into()),
                #[cfg(all(feature = "tree-sitter-typescript", feature = "tree-sitter-javascript"))]
                Language::TypeScript => {
                    let combined = format!(
                        "{}\n{}",
                        tree_sitter_javascript::HIGHLIGHT_QUERY,
                        tree_sitter_typescript::HIGHLIGHTS_QUERY
                    );
                    Some(combined.into())
                }
                #[cfg(all(
                    feature = "tree-sitter-typescript",
                    not(feature = "tree-sitter-javascript")
                ))]
                Language::TypeScript => Some(tree_sitter_typescript::HIGHLIGHTS_QUERY.into()),
                #[cfg(all(
                    feature = "tree-sitter-javascript",
                    not(feature = "tree-sitter-typescript")
                ))]
                Language::TypeScript => Some(tree_sitter_javascript::HIGHLIGHT_QUERY.into()),
                #[cfg(feature = "tree-sitter-python")]
                Language::Python => Some(tree_sitter_python::HIGHLIGHTS_QUERY.into()),
                #[cfg(feature = "tree-sitter-json")]
                Language::Json => Some(tree_sitter_json::HIGHLIGHTS_QUERY.into()),
                #[cfg(feature = "tree-sitter-toml-ng")]
                Language::Toml => Some(tree_sitter_toml_ng::HIGHLIGHTS_QUERY.into()),
                #[cfg(feature = "tree-sitter-md")]
                Language::Markdown => Some(tree_sitter_md::HIGHLIGHT_QUERY_BLOCK.into()),
                #[cfg(feature = "tree-sitter-go")]
                Language::Go => Some(tree_sitter_go::HIGHLIGHTS_QUERY.into()),
                #[cfg(feature = "tree-sitter-c")]
                Language::C => Some(tree_sitter_c::HIGHLIGHT_QUERY.into()),
                #[cfg(all(feature = "tree-sitter-cpp", feature = "tree-sitter-c"))]
                Language::Cpp => {
                    let combined = format!(
                        "{}\n{}",
                        tree_sitter_c::HIGHLIGHT_QUERY,
                        tree_sitter_cpp::HIGHLIGHT_QUERY
                    );
                    Some(combined.into())
                }
                #[cfg(all(feature = "tree-sitter-cpp", not(feature = "tree-sitter-c")))]
                Language::Cpp => Some(tree_sitter_cpp::HIGHLIGHT_QUERY.into()),
                #[cfg(feature = "tree-sitter-java")]
                Language::Java => Some(tree_sitter_java::HIGHLIGHTS_QUERY.into()),
                #[cfg(feature = "tree-sitter-ruby")]
                Language::Ruby => Some(tree_sitter_ruby::HIGHLIGHTS_QUERY.into()),
                #[cfg(feature = "tree-sitter-bash")]
                Language::Bash => Some(tree_sitter_bash::HIGHLIGHT_QUERY.into()),
                #[cfg(feature = "tree-sitter-css")]
                Language::Css => Some(tree_sitter_css::HIGHLIGHTS_QUERY.into()),
                #[cfg(feature = "tree-sitter-html")]
                Language::Html => Some(tree_sitter_html::HIGHLIGHTS_QUERY.into()),
                #[cfg(feature = "tree-sitter-yaml")]
                Language::Yaml => Some(tree_sitter_yaml::HIGHLIGHTS_QUERY.into()),
                #[cfg(feature = "tree-sitter-lua")]
                Language::Lua => Some(tree_sitter_lua::HIGHLIGHTS_QUERY.into()),
                #[cfg(feature = "tree-sitter-zig")]
                Language::Zig => Some(tree_sitter_zig::HIGHLIGHTS_QUERY.into()),
                #[cfg(feature = "tree-sitter-scala")]
                Language::Scala => Some(tree_sitter_scala::HIGHLIGHTS_QUERY.into()),
                #[cfg(feature = "tree-sitter-php")]
                Language::Php => Some(tree_sitter_php::HIGHLIGHTS_QUERY.into()),
                #[cfg(feature = "tree-sitter-ocaml")]
                Language::OCaml => Some(tree_sitter_ocaml::HIGHLIGHTS_QUERY.into()),
                #[cfg(feature = "tree-sitter-sequel")]
                Language::Sql => Some(tree_sitter_sequel::HIGHLIGHTS_QUERY.into()),
                _ => None,
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            None
        }
    }
}

pub fn highlight_color_for_capture(capture_name: &str) -> Hsla {
    match capture_name {
        "keyword"
        | "keyword.control"
        | "keyword.operator"
        | "keyword.function"
        | "keyword.return"
        | "keyword.control.repeat"
        | "keyword.control.conditional"
        | "keyword.control.import"
        | "keyword.control.exception"
        | "keyword.directive"
        | "keyword.modifier"
        | "keyword.type"
        | "keyword.coroutine"
        | "keyword.storage.type"
        | "keyword.storage.modifier"
        | "conditional"
        | "repeat"
        | "include"
        | "exception" => hsla(0.77, 0.75, 0.70, 1.0),

        "type" | "type.builtin" | "type.definition" | "type.qualifier" | "storageclass"
        | "structure" => hsla(0.47, 0.60, 0.65, 1.0),

        "function" | "function.call" | "function.method" | "function.builtin"
        | "function.macro" | "method" | "method.call" | "constructor" => {
            hsla(0.58, 0.65, 0.70, 1.0)
        }

        "string"
        | "string.special"
        | "string.escape"
        | "string.regex"
        | "string.special.url"
        | "string.special.path"
        | "character"
        | "character.special" => hsla(0.25, 0.55, 0.60, 1.0),

        "number" | "float" | "constant.numeric" => hsla(0.08, 0.75, 0.65, 1.0),

        "comment" | "comment.line" | "comment.block" | "comment.documentation" => {
            hsla(0.0, 0.0, 0.45, 1.0)
        }

        "operator" => hsla(0.55, 0.50, 0.70, 1.0),

        "variable" | "variable.parameter" | "variable.builtin" | "variable.member"
        | "parameter" | "field" => hsla(0.0, 0.0, 0.85, 1.0),

        "constant" | "constant.builtin" | "constant.macro" | "boolean" | "define" | "symbol" => {
            hsla(0.08, 0.75, 0.65, 1.0)
        }

        "property" | "property.definition" => hsla(0.55, 0.50, 0.70, 1.0),

        "punctuation" | "punctuation.bracket" | "punctuation.delimiter" | "punctuation.special" => {
            hsla(0.0, 0.0, 0.60, 1.0)
        }

        "attribute" | "label" | "annotation" | "decorator" => hsla(0.12, 0.60, 0.65, 1.0),

        "namespace" | "module" => hsla(0.08, 0.50, 0.70, 1.0),

        "tag" | "tag.builtin" | "tag.delimiter" | "tag.attribute" => hsla(0.0, 0.65, 0.65, 1.0),

        "text.title" | "markup.heading" | "text.strong" | "markup.bold" => {
            hsla(0.58, 0.65, 0.80, 1.0)
        }
        "text.emphasis" | "markup.italic" => hsla(0.25, 0.55, 0.70, 1.0),
        "text.uri" | "markup.link.url" | "markup.link" => hsla(0.55, 0.60, 0.65, 1.0),
        "text.literal" | "markup.raw" => hsla(0.25, 0.55, 0.60, 1.0),

        "embedded" | "injection.content" => hsla(0.0, 0.0, 0.80, 1.0),

        _ => hsla(0.0, 0.0, 0.85, 1.0),
    }
}

#[derive(Clone, Copy)]
struct CollapsedLineSpan {
    start: usize,
    end: usize,
    display_start: usize,
    removed_through: usize,
}

/// A collapsed-fold index stores one interval per effective fold, rather than
/// one integer per document line. Unfolded documents need no allocated index.
struct FoldLineIndex {
    total_lines: usize,
    visible_lines: usize,
    spans: Vec<CollapsedLineSpan>,
}
impl FoldLineIndex {
    fn new(total_lines: usize, folds: &[FoldRange]) -> Self {
        let mut sorted = folds.to_vec();
        sorted.sort_by_key(|fold| fold.start_line);
        let mut spans: Vec<CollapsedLineSpan> = Vec::with_capacity(sorted.len());
        let mut removed = 0;
        for fold in sorted {
            if fold.start_line >= total_lines || fold.end_line <= fold.start_line {
                continue;
            }
            // A fold whose header is hidden by an earlier fold is ineffective,
            // matching the editor's existing nested/overlapping fold semantics.
            if spans.last().is_some_and(|span| fold.start_line <= span.end) {
                continue;
            }
            let end = fold.end_line.min(total_lines.saturating_sub(1));
            let display_start = fold.start_line - removed;
            removed += end - fold.start_line;
            spans.push(CollapsedLineSpan {
                start: fold.start_line,
                end,
                display_start,
                removed_through: removed,
            });
        }
        Self {
            total_lines,
            visible_lines: total_lines - removed,
            spans,
        }
    }
    fn row_for_line(&self, line: usize) -> Option<usize> {
        if line >= self.total_lines {
            return None;
        }
        let preceding = self.spans.partition_point(|span| span.start < line);
        let Some(span) = preceding.checked_sub(1).map(|index| &self.spans[index]) else {
            return Some(line);
        };
        if line <= span.end {
            None
        } else {
            Some(line - span.removed_through)
        }
    }
    fn line_for_row(&self, row: usize) -> Option<usize> {
        if row >= self.visible_lines {
            return None;
        }
        let preceding = self.spans.partition_point(|span| span.display_start < row);
        let removed = preceding
            .checked_sub(1)
            .map_or(0, |index| self.spans[index].removed_through);
        Some(row + removed)
    }
    fn is_header(&self, line: usize) -> bool {
        self.spans
            .binary_search_by_key(&line, |span| span.start)
            .is_ok()
    }
}

#[derive(Clone)]
enum DisplayLineIndex {
    Unfolded(usize),
    Folded(Arc<FoldLineIndex>),
    FoldedWithEof(Arc<FoldLineIndex>),
}
impl DisplayLineIndex {
    fn len(&self) -> usize {
        match self {
            Self::Unfolded(lines) => *lines,
            Self::Folded(index) => index.visible_lines,
            Self::FoldedWithEof(index) => index.visible_lines + 1,
        }
    }
    fn line_for_row(&self, row: usize) -> Option<usize> {
        match self {
            Self::Unfolded(lines) => (row < *lines).then_some(row),
            Self::Folded(index) => index.line_for_row(row),
            Self::FoldedWithEof(index) => {
                if row == index.visible_lines {
                    Some(index.total_lines)
                } else {
                    index.line_for_row(row)
                }
            }
        }
    }
    fn row_for_line(&self, line: usize) -> Option<usize> {
        match self {
            Self::Unfolded(lines) => (line < *lines).then_some(line),
            Self::Folded(index) => index.row_for_line(line),
            Self::FoldedWithEof(index) => {
                if line == index.total_lines {
                    Some(index.visible_lines)
                } else {
                    index.row_for_line(line)
                }
            }
        }
    }
    fn visible_range(&self, range: Range<usize>) -> Vec<usize> {
        range.filter_map(|row| self.line_for_row(row)).collect()
    }
    fn is_fold_header(&self, line: usize) -> bool {
        match self {
            Self::Unfolded(_) => false,
            Self::Folded(index) | Self::FoldedWithEof(index) => index.is_header(line),
        }
    }
}

#[derive(Clone, PartialEq)]
struct EditorGeometryKey {
    document: AccessibilityId,
    bounds: Bounds<Pixels>,
    viewport: Bounds<Pixels>,
    scroll_x: Pixels,
    font_size: Pixels,
    gutter: Pixels,
    fold_revision: u64,
    first_row: usize,
    last_row: usize,
}

pub struct EditorState {
    focus_handle: FocusHandle,
    accessibility_id: AccessibilityId,
    rope: Rope,
    cursor: Position,
    selection: Option<Selection>,

    undo_stack: Vec<EditOp>,
    redo_stack: Vec<EditOp>,

    file_path: Option<PathBuf>,
    is_modified: bool,
    content_version: u64,
    accessibility_document: Option<(u64, Arc<AccessibilityTextDocument>)>,
    accessibility_preparation_task: Option<Task<()>>,
    accessibility_geometry: Option<(EditorGeometryKey, Arc<AccessibilityTextGeometry>)>,
    accessibility_reveal: Option<(AccessibilityId, Range<usize>, AccessibilityTextAlignment)>,
    reveal_eof: bool,

    parser: Parser,
    syntax_tree: Option<Tree>,
    highlight_query: Option<Query>,
    language: Language,

    scroll_handle: ScrollHandle,
    scroll_offset_x: Pixels,
    pending_cursor_scroll: bool,
    max_line_width: Pixels,
    line_layouts: HashMap<usize, ShapedLine>,
    line_content_hashes: HashMap<usize, u64>,
    line_geometry_compatible: HashMap<usize, bool>,
    line_text_runs: HashMap<usize, Vec<TextRun>>,
    line_geometry_candidates: HashMap<usize, Vec<(Pixels, usize)>>,
    line_native_geometry: HashMap<usize, (Vec<Range<usize>>, Option<Arc<LineTextGeometry>>)>,
    cached_highlight_spans: Vec<HighlightSpan>,
    highlight_cache_version: u64,
    highlight_cache_first_line: usize,
    highlight_cache_last_line: usize,
    last_bounds: Option<Bounds<Pixels>>,

    is_selecting: bool,
    dragging_h_scrollbar: bool,
    last_mouse_pos: Option<Point<Pixels>>,
    last_mouse_gutter_width: Pixels,
    autoscroll_task: Option<Task<()>>,
    last_click_time: Option<web_time::Instant>,

    marked_range: Option<Range<usize>>,

    pub show_line_numbers: bool,
    tab_size: usize,
    read_only: bool,
    disabled: bool,

    pub font_size: Pixels,
    pub line_height: Pixels,
    pub font_family_override: Option<SharedString>,

    cursor_visible: bool,
    blink_task: Option<Task<()>>,
    blink_window: Option<AnyWindowHandle>,
    blink_subscriptions: Vec<Subscription>,
    blink_paint_epoch: u64,
    last_cursor_move: web_time::Instant,
    last_blink_cursor: Position,

    overlay_active_check: Option<Box<dyn Fn(&App) -> bool + 'static>>,

    reparse_task: Option<Task<()>>,
    search_task: Option<Task<()>>,

    search_query: String,
    search_matches: Vec<(usize, usize)>,
    current_match_idx: Option<usize>,
    search_case_sensitive: bool,
    search_use_regex: bool,

    pub cursor_color_override: Option<Hsla>,
    pub selection_color_override: Option<Hsla>,
    pub line_number_color_override: Option<Hsla>,
    pub line_number_active_color_override: Option<Hsla>,
    pub gutter_bg_override: Option<Hsla>,
    pub search_match_color_overrides: Option<(Hsla, Hsla)>,
    pub current_line_color_override: Option<Hsla>,
    pub bracket_match_color_override: Option<Hsla>,
    pub word_highlight_color_override: Option<Hsla>,
    pub indent_guide_color_override: Option<Hsla>,
    pub indent_guide_active_color_override: Option<Hsla>,
    pub fold_marker_color_override: Option<Hsla>,
    pub diagnostic_error_color: Option<Hsla>,
    pub diagnostic_warning_color: Option<Hsla>,
    pub diagnostic_info_color: Option<Hsla>,
    pub diagnostic_hint_color: Option<Hsla>,
    pub syntax_color_fn: Option<Box<dyn Fn(&str) -> Hsla>>,

    fold_ranges: Vec<FoldRange>,
    folded: Vec<FoldRange>,
    fold_line_index: Option<Arc<FoldLineIndex>>,
    fold_layout_revision: u64,

    diagnostics: Vec<EditorDiagnostic>,
}

#[derive(Debug, Clone)]
pub struct EditorDiagnostic {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub severity: DiagnosticSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

impl DiagnosticSeverity {
    /// Stable severity key for content-safe diagnostics.
    pub fn to_text(&self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Information => "information",
            Self::Hint => "hint",
        }
    }
}

impl EditorDiagnostic {
    pub fn line_span(&self) -> u32 {
        self.end_line.saturating_sub(self.start_line) + 1
    }

    pub fn message_len_bytes(&self) -> usize {
        self.message.len()
    }

    /// Content-safe diagnostic summary that never includes the message text.
    pub fn to_text(&self) -> String {
        format!(
            "editor_diagnostic(severity={}, start_line={}, start_col={}, end_line={}, end_col={}, line_span={}, message_len_bytes={})",
            self.severity.to_text(),
            self.start_line,
            self.start_col,
            self.end_line,
            self.end_col,
            self.line_span(),
            self.message_len_bytes()
        )
    }
}

impl EditorState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let parser = Parser::new();

        Self {
            focus_handle: cx.focus_handle(),
            accessibility_id: AccessibilityId::new(),
            rope: Rope::from_str("\n"),
            cursor: Position::zero(),
            selection: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            file_path: None,
            is_modified: false,
            content_version: 0,
            accessibility_document: None,
            accessibility_preparation_task: None,
            accessibility_geometry: None,
            accessibility_reveal: None,
            reveal_eof: false,
            parser,
            syntax_tree: None,
            highlight_query: None,
            language: Language::Plain,
            scroll_handle: ScrollHandle::new(),
            scroll_offset_x: px(0.0),
            pending_cursor_scroll: false,
            max_line_width: px(0.0),
            line_layouts: HashMap::new(),
            line_content_hashes: HashMap::new(),
            line_geometry_compatible: HashMap::new(),
            line_text_runs: HashMap::new(),
            line_geometry_candidates: HashMap::new(),
            line_native_geometry: HashMap::new(),
            cached_highlight_spans: Vec::new(),
            highlight_cache_version: u64::MAX,
            highlight_cache_first_line: 0,
            highlight_cache_last_line: 0,
            last_bounds: None,
            is_selecting: false,
            dragging_h_scrollbar: false,
            last_mouse_pos: None,
            last_mouse_gutter_width: px(80.0),
            autoscroll_task: None,
            last_click_time: None,
            marked_range: None,
            show_line_numbers: true,
            tab_size: 4,
            read_only: false,
            disabled: false,
            font_size: px(14.0),
            line_height: px(20.0),
            font_family_override: None,
            cursor_visible: true,
            blink_task: None,
            blink_window: None,
            blink_subscriptions: Vec::new(),
            blink_paint_epoch: 0,
            last_cursor_move: web_time::Instant::now(),
            last_blink_cursor: Position::zero(),
            overlay_active_check: None,
            reparse_task: None,
            search_task: None,
            search_query: String::new(),
            search_matches: Vec::new(),
            current_match_idx: None,
            search_case_sensitive: false,
            search_use_regex: false,
            cursor_color_override: None,
            selection_color_override: None,
            line_number_color_override: None,
            line_number_active_color_override: None,
            gutter_bg_override: None,
            search_match_color_overrides: None,
            current_line_color_override: None,
            bracket_match_color_override: None,
            word_highlight_color_override: None,
            indent_guide_color_override: None,
            indent_guide_active_color_override: None,
            fold_marker_color_override: None,
            diagnostic_error_color: None,
            diagnostic_warning_color: None,
            diagnostic_info_color: None,
            diagnostic_hint_color: None,
            syntax_color_fn: None,
            fold_ranges: Vec::new(),
            folded: Vec::new(),
            fold_line_index: None,
            fold_layout_revision: 0,
            diagnostics: Vec::new(),
        }
    }

    pub fn set_cursor_position(&mut self, line: usize, col: usize, cx: &mut Context<Self>) {
        let max_line = self.total_lines().saturating_sub(1);
        self.cursor.line = line.min(max_line);
        let line_len = self.line_len(self.cursor.line);
        self.cursor.col = col.min(line_len);
        self.clamp_cursor();
        self.selection = None;
        self.reset_cursor_blink(cx);
        self.ensure_cursor_visible(cx);
    }

    /// Set selection using UTF-8 byte offsets. Reversed selections retain their
    /// anchor and focus; invalid offsets leave selection and history unchanged.
    /// This also finishes any marked-text range without editing its contents.
    pub fn set_selection_bytes(
        &mut self,
        anchor: usize,
        focus: usize,
        cx: &mut Context<Self>,
    ) -> Result<(), &'static str> {
        for offset in [anchor, focus] {
            if offset > self.rope.len_bytes()
                || self.rope.char_to_byte(self.rope.byte_to_char(offset)) != offset
            {
                return Err("selection offset must be a valid UTF-8 character boundary");
            }
        }
        let anchor = self.byte_offset_to_pos(anchor);
        let focus = self.byte_offset_to_pos(focus);
        self.cursor = focus;
        self.selection = (anchor != focus).then(|| Selection::new(anchor, focus));
        self.marked_range = None;
        self.reset_cursor_blink(cx);
        self.ensure_cursor_visible(cx);
        cx.notify();
        Ok(())
    }

    /// The current selection's UTF-8 byte anchor and focus, or the caret twice.
    pub fn selection_bytes(&self) -> (usize, usize) {
        self.selection.as_ref().map_or_else(
            || {
                let caret = self.pos_to_byte_offset(self.cursor);
                (caret, caret)
            },
            |selection| {
                (
                    self.pos_to_byte_offset(selection.anchor),
                    self.pos_to_byte_offset(selection.cursor),
                )
            },
        )
    }

    pub fn set_font_size(&mut self, size: f32, cx: &mut Context<Self>) {
        self.font_size = px(size);
        self.line_height = px((size * 1.5).round());
        self.line_layouts.clear();
        self.line_content_hashes.clear();
        self.accessibility_geometry = None;
        self.line_geometry_compatible.clear();
        self.line_text_runs.clear();
        self.line_geometry_candidates.clear();
        self.line_native_geometry.clear();
        cx.notify();
    }

    pub fn set_font_family(&mut self, family: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.font_family_override = Some(family.into());
        self.line_layouts.clear();
        self.line_content_hashes.clear();
        self.accessibility_geometry = None;
        self.line_geometry_compatible.clear();
        self.line_text_runs.clear();
        self.line_geometry_candidates.clear();
        self.line_native_geometry.clear();
        cx.notify();
    }

    fn reset_cursor_blink(&mut self, _: &mut Context<Self>) {
        self.cursor_visible = true;
        self.last_cursor_move = web_time::Instant::now();
        // A programmatic selection on an unfocused editor must not start a timer.
        // The next focused paint restarts the caret's complete visible interval.
        self.blink_task = None;
    }

    fn bind_cursor_blink(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.blink_window == Some(window.window_handle()) {
            return;
        }
        self.blink_task = None;
        self.blink_subscriptions.clear();
        self.blink_window = Some(window.window_handle());
        let focus = self.focus_handle.clone();
        self.blink_subscriptions
            .push(cx.on_blur(&focus, window, |state, _, _| {
                state.blink_task = None;
                state.cursor_visible = true;
            }));
        self.blink_subscriptions
            .push(cx.observe_window_activation(window, |state, window, _| {
                if !window.is_window_active() {
                    state.blink_task = None;
                    state.cursor_visible = true;
                }
            }));
    }

    fn paint_cursor_blink(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.blink_paint_epoch = self.blink_paint_epoch.wrapping_add(1);
        if !self.focus_handle.is_focused(window)
            || self.disabled
            || !window.is_window_active()
            || !window.is_window_visible()
            || window.reduce_motion()
        {
            self.blink_task = None;
            self.cursor_visible = true;
            return;
        }
        if self.blink_task.is_some() {
            return;
        }
        let this = cx.weak_entity();
        let mut last_paint = self.blink_paint_epoch.wrapping_sub(1);
        self.blink_task = Some(window.spawn(cx, async move |cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                let keep_blinking = this
                    .update_in(cx, |state, window, cx| {
                        // A retained state whose editor stopped painting gets at most
                        // one pending check, never a permanent idle notification loop.
                        if !state.focus_handle.is_focused(window)
                            || !window.is_window_active()
                            || !window.is_window_visible()
                            || window.reduce_motion()
                            || last_paint == state.blink_paint_epoch
                        {
                            state.blink_task = None;
                            state.cursor_visible = true;
                            return false;
                        }
                        last_paint = state.blink_paint_epoch;
                        state.cursor_visible = !state.cursor_visible;
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep_blinking {
                    break;
                }
            }
        }));
    }

    pub fn set_diagnostics(&mut self, diagnostics: Vec<EditorDiagnostic>, cx: &mut Context<Self>) {
        self.diagnostics = diagnostics;
        cx.notify();
    }

    pub fn diagnostics(&self) -> &[EditorDiagnostic] {
        &self.diagnostics
    }

    pub fn diagnostics_at_line(&self, line: usize) -> Vec<&EditorDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.start_line as usize <= line && line <= d.end_line as usize)
            .collect()
    }

    pub fn content_len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    fn prepare_accessibility_document(&mut self, cx: &mut Context<Self>) {
        if self
            .accessibility_document
            .as_ref()
            .is_some_and(|(revision, _)| *revision == self.content_version)
        {
            return;
        }
        let revision = self.content_version;
        let executor = cx.background_executor().clone();
        if self.rope.len_bytes() <= INLINE_ACCESSIBILITY_DOCUMENT_BYTES {
            self.accessibility_document = Some((
                revision,
                AccessibilityTextDocument::with_reclaim_executor(self.rope.to_string(), &executor),
            ));
            return;
        }
        if self.accessibility_preparation_task.is_some() {
            return; // One worker coalesces later revisions without cloning text on the UI thread.
        }
        let rope = self.rope.clone();
        self.accessibility_preparation_task = Some(cx.spawn(async move |state, cx| {
            let worker = executor.clone();
            let document = executor
                .spawn(async move {
                    AccessibilityTextDocument::with_reclaim_executor(rope.to_string(), &worker)
                })
                .await;
            let _ = state.update(cx, |state, cx| {
                state.accessibility_preparation_task = None;
                if state.content_version == revision {
                    state.accessibility_document = Some((revision, document));
                }
                state.prepare_accessibility_document(cx);
                cx.notify();
            });
        }));
    }

    fn prepared_accessibility_document(&self) -> Option<Arc<AccessibilityTextDocument>> {
        self.accessibility_document
            .as_ref()
            .filter(|(revision, _)| *revision == self.content_version)
            .map(|(_, document)| document.clone())
    }

    fn set_accessibility_selection(
        &mut self,
        document_id: AccessibilityId,
        anchor: usize,
        focus: usize,
        cx: &mut Context<Self>,
    ) -> Result<(), &'static str> {
        if self.disabled {
            return Err("text is disabled");
        }
        let selection = AccessibilityTextSelection { anchor, focus };
        if !self
            .prepared_accessibility_document()
            .is_some_and(|document| {
                document.id() == document_id && document.contains_selection(selection)
            })
        {
            return Err("selection requires the originating current prepared document");
        }
        self.set_selection_bytes(anchor, focus, cx)
    }

    fn accessibility_value(&self) -> String {
        self.rope
            .chars()
            .take(MAX_ACCESSIBILITY_VALUE_CHARS)
            .collect()
    }

    fn checked_accessibility_range(
        &self,
        id: AccessibilityId,
        start: usize,
        end: usize,
    ) -> Result<Range<usize>, &'static str> {
        if self.disabled {
            return Err("text is disabled");
        }
        if start > end
            || !self
                .prepared_accessibility_document()
                .is_some_and(|document| {
                    document.id() == id
                        && document.contains_selection(AccessibilityTextSelection {
                            anchor: start,
                            focus: end,
                        })
                })
        {
            return Err("text action requires the originating current prepared document");
        }
        Ok(start..end)
    }

    fn replace_accessibility_text(
        &mut self,
        id: AccessibilityId,
        start: usize,
        end: usize,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), &'static str> {
        if self.read_only || self.disabled {
            return Err("text is read-only");
        }
        let range = self.checked_accessibility_range(id, start, end)?;
        self.marked_range = None;
        self.replace_input_range(range, text, false, cx);
        self.ensure_cursor_visible(cx);
        cx.notify();
        Ok(())
    }

    fn accessibility_clipboard(
        &mut self,
        action: AccessibilityAction,
        id: AccessibilityId,
        anchor: usize,
        focus: usize,
        cx: &mut Context<Self>,
    ) -> Result<(), &'static str> {
        let range = self.checked_accessibility_range(id, anchor.min(focus), anchor.max(focus))?;
        match action {
            AccessibilityAction::CopyText => cx.write_to_clipboard(ClipboardItem::new_string(
                self.rope.byte_slice(range).to_string(),
            )),
            AccessibilityAction::CutText => {
                if self.read_only || self.disabled {
                    return Err("text is read-only");
                }
                cx.write_to_clipboard(ClipboardItem::new_string(
                    self.rope.byte_slice(range.clone()).to_string(),
                ));
                self.replace_accessibility_text(id, range.start, range.end, "", cx)?;
            }
            AccessibilityAction::PasteText => {
                if self.read_only || self.disabled {
                    return Err("text is read-only");
                }
                let text = cx
                    .read_from_clipboard()
                    .map_err(|_| "clipboard unavailable")?
                    .and_then(|item| item.text())
                    .ok_or("clipboard has no text")?;
                self.replace_accessibility_text(id, range.start, range.end, &text, cx)?;
            }
            _ => return Err("unsupported clipboard operation"),
        }
        Ok(())
    }

    fn reveal_accessibility_text(
        &mut self,
        id: AccessibilityId,
        start: usize,
        end: usize,
        alignment: AccessibilityTextAlignment,
        cx: &mut Context<Self>,
    ) -> Result<(), &'static str> {
        let range = self.checked_accessibility_range(id, start, end)?;
        let byte = if matches!(
            alignment,
            AccessibilityTextAlignment::Bottom
                | AccessibilityTextAlignment::BottomRight
                | AccessibilityTextAlignment::Right
        ) {
            end
        } else {
            start
        };
        let pos = self.byte_offset_to_pos(byte);
        self.reveal_eof |= pos.line >= self.total_lines();
        if self.display_line_index().row_for_line(pos.line).is_none() {
            self.folded
                .retain(|fold| !(pos.line > fold.start_line && pos.line <= fold.end_line));
            self.rebuild_fold_line_index();
            self.invalidate_all_caches();
        }
        let row = self
            .buffer_line_to_display_row(pos.line)
            .ok_or("text row is not displayed")?;
        let viewport = self.scroll_handle.bounds();
        let target = px(12.0) + self.line_height * row as f32;
        let offset = self.scroll_handle.offset();
        let top = -offset.y;
        let height = viewport.size.height;
        let y = match alignment {
            AccessibilityTextAlignment::Top | AccessibilityTextAlignment::TopLeft => -target,
            AccessibilityTextAlignment::Bottom | AccessibilityTextAlignment::BottomRight => {
                -(target + self.line_height - height)
            }
            _ if target < top => -target,
            _ if target + self.line_height > top + height => -(target + self.line_height - height),
            _ => offset.y,
        }
        .min(px(0.0));
        // Layout will clamp against the newly expanded document, not old folds.
        self.scroll_handle.set_offset(point(offset.x, y));
        self.accessibility_reveal = Some((id, range, alignment));
        cx.notify();
        Ok(())
    }

    fn finish_accessibility_reveal(&mut self, cx: &mut Context<Self>) {
        let Some((id, range, alignment)) = self.accessibility_reveal.take() else {
            return;
        };
        if self
            .checked_accessibility_range(id, range.start, range.end)
            .is_err()
        {
            return;
        }
        let byte = if matches!(
            alignment,
            AccessibilityTextAlignment::Bottom
                | AccessibilityTextAlignment::BottomRight
                | AccessibilityTextAlignment::Right
        ) {
            range.end
        } else {
            range.start
        };
        let pos = self.byte_offset_to_pos(byte);
        let x = self.line_caret_x(pos.line, pos.col);
        let width = (self.scroll_handle.bounds().size.width
            - if self.show_line_numbers {
                px(80.0)
            } else {
                px(12.0)
            }
            - px(20.0))
        .max(px(1.0));
        let before = self.scroll_offset_x;
        self.scroll_offset_x = match alignment {
            AccessibilityTextAlignment::Left | AccessibilityTextAlignment::TopLeft => x,
            AccessibilityTextAlignment::Right | AccessibilityTextAlignment::BottomRight => {
                x - width
            }
            _ if x < before => x,
            _ if x > before + width => x - width,
            _ => before,
        }
        .max(px(0.0));
        if before != self.scroll_offset_x {
            cx.notify();
        }
    }

    // Equivalent to LTR layout caret placement, with bounded binary searches
    // rather than walking every preceding glyph for every selectable unit.
    fn shaped_x(layout: &ShapedLine, byte: usize) -> Pixels {
        layout
            .runs
            .iter()
            .find_map(|run| {
                let index = run.glyphs.partition_point(|glyph| glyph.index < byte);
                run.glyphs.get(index).map(|glyph| glyph.position.x)
            })
            .unwrap_or(layout.width)
    }

    fn native_geometry_for_line(&self, line: usize) -> Option<&LineTextGeometry> {
        self.line_native_geometry.get(&line)?.1.as_deref()
    }

    fn line_caret_x(&self, line: usize, byte: usize) -> Pixels {
        self.line_layouts.get(&line).map_or(px(0.0), |layout| {
            self.native_geometry_for_line(line)
                .and_then(|geometry| geometry.caret_for_byte(byte, layout.len()))
                .unwrap_or_else(|| layout.x_for_index(byte))
        })
    }

    fn line_range_rectangles(&self, line: usize, range: Range<usize>) -> Vec<Range<Pixels>> {
        if let Some(geometry) = self.native_geometry_for_line(line) {
            return geometry.rectangles_for_bytes(range);
        }
        let a = self.line_caret_x(line, range.start);
        let b = self.line_caret_x(line, range.end);
        vec![a.min(b)..a.max(b)]
    }

    fn bounds_for_byte_range(&self, range: Range<usize>) -> Option<Bounds<Pixels>> {
        self.bounds_for_byte_range_with_actual_range(range)
            .map(|(mut bounds, _)| {
                bounds.size.width = bounds.size.width.max(px(1.0));
                bounds
            })
    }

    fn bounds_for_byte_range_with_actual_range(
        &self,
        range: Range<usize>,
    ) -> Option<(Bounds<Pixels>, Range<usize>)> {
        if range.start > range.end || range.end > self.rope.len_bytes() {
            return None;
        }
        let bounds = self.last_bounds?;
        let start = self.byte_offset_to_pos(range.start);
        let row = self.buffer_line_to_display_row(start.line)?;
        let y = bounds.top() + px(12.0) + self.line_height * row as f32;
        let viewport = self.scroll_handle.bounds();
        if y + self.line_height < viewport.top() || y >= viewport.bottom() {
            return None;
        }
        let gutter = if self.show_line_numbers {
            px(80.0)
        } else {
            px(12.0)
        };
        let line_start = self.rope.line_to_byte(start.line);
        let line_length = self.line_len(start.line);
        let line_end = line_start + self.rope.line(start.line).len_bytes();
        let first_end = range.end.min(line_end);
        let local_start = start.col.min(line_length);
        let local_end = (first_end - line_start).min(line_length);
        let native = self.native_geometry_for_line(start.line);
        let (actual, rectangle) = if local_start == local_end {
            let caret_byte = native
                .and_then(|geometry| geometry.cluster_for_byte(local_start))
                .map_or(local_start, |cluster| cluster.bytes.start);
            let x = self.line_caret_x(start.line, caret_byte);
            (
                if range.is_empty() {
                    line_start + caret_byte..line_start + caret_byte
                } else {
                    range.start..first_end
                },
                x..x,
            )
        } else {
            let (mut actual, rectangle) = native
                .and_then(|geometry| geometry.first_fragment_for_bytes(local_start..local_end))
                .unwrap_or_else(|| {
                    let text = self.line_text(start.line);
                    let actual_start = text
                        .grapheme_indices(true)
                        .map(|(byte, _)| byte)
                        .take_while(|byte| *byte <= local_start)
                        .last()
                        .unwrap_or(0);
                    let actual_end = text
                        .grapheme_indices(true)
                        .map(|(byte, _)| byte)
                        .find(|byte| *byte >= local_end)
                        .unwrap_or(text.len());
                    let a = self.line_caret_x(start.line, actual_start);
                    let b = self.line_caret_x(start.line, actual_end);
                    (actual_start..actual_end, a.min(b)..a.max(b))
                });
            // A line terminator has no ink but belongs to this line fragment.
            // Including it lets AppKit advance directly to the following line.
            if actual.end == line_length && first_end > line_start + line_length {
                actual.end = first_end - line_start;
            }
            (
                line_start + actual.start..line_start + actual.end,
                rectangle,
            )
        };
        let width = rectangle.end - rectangle.start;
        let x = rectangle.start;
        let x = bounds.left() + gutter + x - self.scroll_offset_x;
        Some((
            Bounds::new(point(x, y), size(width, self.line_height)),
            actual,
        ))
    }

    fn prepare_accessibility_geometry(
        &mut self,
        bounds: Bounds<Pixels>,
        first_row: usize,
        last_row: usize,
        visible_lines: &[usize],
        text_system: &WindowTextSystem,
    ) -> Option<Arc<AccessibilityTextGeometry>> {
        let document = self.prepared_accessibility_document()?;
        let gutter = if self.show_line_numbers {
            px(80.0)
        } else {
            px(12.0)
        };
        let key = EditorGeometryKey {
            document: document.id(),
            bounds,
            viewport: self.scroll_handle.bounds(),
            scroll_x: self.scroll_offset_x,
            font_size: self.font_size,
            gutter,
            fold_revision: self.fold_layout_revision,
            first_row,
            last_row,
        };
        let native_caret_covered =
            self.native_geometry_for_line(self.cursor.line)
                .is_none_or(|geometry| {
                    self.line_layouts
                        .get(&self.cursor.line)
                        .is_none_or(|layout| {
                            geometry
                                .caret_for_byte(self.cursor.col, layout.len())
                                .is_some()
                        })
                });
        if let Some((old, geometry)) = &self.accessibility_geometry
            && *old == key
            && self.accessibility_reveal.is_none()
            && native_caret_covered
        {
            return Some(geometry.clone());
        }
        let gutter = if self.show_line_numbers {
            px(80.0)
        } else {
            px(12.0)
        };
        let width = (key.viewport.size.width - gutter).max(px(1.0));
        let mut runs = Vec::new();
        for &line in visible_lines {
            let start = self.rope.line_to_byte(line);
            let line_bytes = self.rope.line(line).len_bytes();
            let row = self.buffer_line_to_display_row(line)?;
            let y = bounds.top() + px(12.0) + self.line_height * row as f32;
            if y + self.line_height < key.viewport.top() || y >= key.viewport.bottom() {
                continue;
            }
            let layout = self.line_layouts.get(&line);
            let compatible = layout.is_none_or(|layout| {
                *self
                    .line_geometry_compatible
                    .entry(line)
                    .or_insert_with(|| {
                        let mut previous = None;
                        layout.runs.iter().flat_map(|run| &run.glyphs).all(|glyph| {
                            let valid = previous.is_none_or(|(index, x)| {
                                glyph.index >= index && glyph.position.x >= x
                            });
                            previous = Some((glyph.index, glyph.position.x));
                            valid
                        })
                    })
            });
            let mut requested_runs = Vec::new();
            if let Some(layout) = layout {
                let candidates = self
                    .line_geometry_candidates
                    .entry(line)
                    .or_insert_with(|| {
                        let mut candidates = layout
                            .runs
                            .iter()
                            .flat_map(|run| &run.glyphs)
                            .map(|glyph| (glyph.position.x, glyph.index))
                            .collect::<Vec<_>>();
                        if !candidates.windows(2).all(|pair| pair[0].0 <= pair[1].0) {
                            candidates.sort_unstable_by(|a, b| {
                                a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)
                            });
                        }
                        candidates
                    });
                let first = candidates
                    .partition_point(|(x, _)| *x < self.scroll_offset_x)
                    .saturating_sub(1);
                let last =
                    (candidates.partition_point(|(x, _)| *x <= self.scroll_offset_x + width) + 1)
                        .min(candidates.len());
                for &(_, byte) in &candidates[first..last] {
                    requested_runs
                        .extend(document.runs_for_bytes(start + byte..start + byte).take(1));
                }
            } else {
                requested_runs.extend(document.runs_for_bytes(start..start + line_bytes));
            }
            // A pending reveal needs its actual caret even before horizontal
            // scrolling mounts it. One extra bounded run also keeps EOF honest.
            let caret_byte = self
                .accessibility_reveal
                .as_ref()
                .map(|(_, range, alignment)| {
                    if matches!(
                        alignment,
                        AccessibilityTextAlignment::Bottom
                            | AccessibilityTextAlignment::BottomRight
                            | AccessibilityTextAlignment::Right
                    ) {
                        range.end
                    } else {
                        range.start
                    }
                })
                .filter(|byte| self.byte_offset_to_pos(*byte).line == line);
            if let Some(byte) = caret_byte {
                requested_runs.extend(document.runs_for_bytes(byte..byte).take(1));
            }
            if self.cursor.line == line {
                let byte = self.pos_to_byte_offset(self.cursor);
                requested_runs.extend(document.runs_for_bytes(byte..byte).take(1));
            }
            let last_byte = start + layout.map_or(0, ShapedLine::len);
            requested_runs.extend(document.runs_for_bytes(last_byte..last_byte).take(1));
            requested_runs.retain(|run| run.hard_line == line);
            requested_runs.sort_unstable_by_key(|run| run.bytes.start);
            requested_runs.dedup_by_key(|run| run.bytes.start);
            let native = layout.and_then(|layout| {
                let requested = requested_runs
                    .iter()
                    .map(|run| {
                        (run.bytes.start - start).min(layout.len())
                            ..(run.bytes.end - start).min(layout.len())
                    })
                    .collect::<Vec<_>>();
                let entry = self
                    .line_native_geometry
                    .entry(line)
                    .or_insert_with(|| (Vec::new(), None));
                if entry.0 != requested {
                    entry.1 = self.line_text_runs.get(&line).and_then(|fonts| {
                        text_system
                            .line_text_geometry(&layout.text, self.font_size, fonts, &requested)
                            .map(Arc::new)
                    });
                    entry.0 = requested;
                }
                entry.1.clone()
            });
            for run in requested_runs {
                let mut byte = run.bytes.start - start;
                let mut edges = Vec::with_capacity(run.character_lengths.len());
                let mut valid = true;
                for &length in run.character_lengths {
                    let pair = if let Some(native) = &native {
                        if byte >= layout.map_or(0, ShapedLine::len) {
                            Some((native.end_caret, native.end_caret))
                        } else {
                            native
                                .cluster_for_byte(byte)
                                .filter(|cluster| {
                                    cluster.right_to_left
                                        == (run.direction
                                            == AccessibilityTextDirection::RightToLeft)
                                })
                                .map(|cluster| (cluster.leading, cluster.trailing))
                        }
                    } else if compatible && run.direction == AccessibilityTextDirection::LeftToRight
                    {
                        Some((
                            layout.map_or(px(0.0), |layout| Self::shaped_x(layout, byte)),
                            layout.map_or(px(0.0), |layout| {
                                Self::shaped_x(layout, byte + usize::from(length))
                            }),
                        ))
                    } else {
                        None
                    };
                    let Some(pair) = pair else {
                        valid = false;
                        break;
                    };
                    edges.push(pair);
                    byte += usize::from(length);
                }
                if !valid {
                    continue;
                }
                let caret = native.as_ref().map_or(px(0.0), |native| native.end_caret);
                let left = edges
                    .iter()
                    .map(|&(a, b)| a.min(b))
                    .reduce(Pixels::min)
                    .unwrap_or(caret);
                let right = edges
                    .iter()
                    .map(|&(a, b)| a.max(b))
                    .reduce(Pixels::max)
                    .unwrap_or(caret);
                if right < self.scroll_offset_x || left > self.scroll_offset_x + width {
                    continue;
                }
                let rtl = run.direction == AccessibilityTextDirection::RightToLeft;
                let positions = edges
                    .iter()
                    .map(|(a, b)| {
                        f32::from(if rtl {
                            right - (*a).max(*b)
                        } else {
                            (*a).min(*b) - left
                        })
                    })
                    .collect::<Vec<_>>();
                let widths = edges
                    .iter()
                    .map(|(a, b)| f32::from((*b - *a).abs()))
                    .collect::<Vec<_>>();
                runs.push(AccessibilityTextRunGeometry {
                    run_id: run.id,
                    bounds: AccessibilityRect::from_bounds(Bounds::new(
                        point(bounds.left() + gutter + left - self.scroll_offset_x, y),
                        size((right - left).max(px(1.0)), self.line_height),
                    )),
                    direction: run.direction,
                    character_positions: positions.into(),
                    character_widths: widths.into(),
                });
            }
        }
        let geometry = AccessibilityTextGeometry::new(&document, runs).ok()?;
        self.accessibility_geometry = Some((key, geometry.clone()));
        Some(geometry)
    }

    pub fn has_selection(&self) -> bool {
        self.selection.is_some()
    }

    pub fn selection_is_empty(&self) -> bool {
        self.selection
            .as_ref()
            .map(|selection| selection.is_empty())
            .unwrap_or(true)
    }

    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    pub fn redo_depth(&self) -> usize {
        self.redo_stack.len()
    }

    pub fn has_file_path(&self) -> bool {
        self.file_path.is_some()
    }

    pub fn has_syntax_tree(&self) -> bool {
        self.syntax_tree.is_some()
    }

    pub fn has_highlight_query(&self) -> bool {
        self.highlight_query.is_some()
    }

    pub fn tab_size(&self) -> usize {
        self.tab_size
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Toggle user/native editing. Pending IME composition is finished without
    /// committing another edit; selection and reading remain available.
    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        if self.read_only != read_only {
            self.read_only = read_only;
            self.marked_range = None;
            cx.notify();
        }
    }

    /// Whether user input and native text actions are disabled.
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Disable input without changing document content or editing history.
    /// The next paint removes focus/tab participation and native actions.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            if disabled {
                self.marked_range = None;
                self.blink_task = None;
                self.cursor_visible = true;
                self.autoscroll_task = None;
                self.is_selecting = false;
                self.dragging_h_scrollbar = false;
                self.pending_cursor_scroll = false;
                self.accessibility_reveal = None;
            }
            cx.notify();
        }
    }

    pub fn search_query_len_bytes(&self) -> usize {
        self.search_query.len()
    }

    pub fn has_search_query(&self) -> bool {
        !self.search_query.is_empty()
    }

    pub fn has_current_match(&self) -> bool {
        self.current_match_idx.is_some()
    }

    pub fn fold_range_count(&self) -> usize {
        self.fold_ranges.len()
    }

    pub fn folded_range_count(&self) -> usize {
        self.folded.len()
    }

    pub fn diagnostic_count(&self) -> usize {
        self.diagnostics.len()
    }

    pub fn diagnostic_count_by_severity(&self, severity: DiagnosticSeverity) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == severity)
            .count()
    }

    pub fn has_visual_overrides(&self) -> bool {
        self.cursor_color_override.is_some()
            || self.selection_color_override.is_some()
            || self.line_number_color_override.is_some()
            || self.line_number_active_color_override.is_some()
            || self.gutter_bg_override.is_some()
            || self.search_match_color_overrides.is_some()
            || self.current_line_color_override.is_some()
            || self.bracket_match_color_override.is_some()
            || self.word_highlight_color_override.is_some()
            || self.indent_guide_color_override.is_some()
            || self.indent_guide_active_color_override.is_some()
            || self.fold_marker_color_override.is_some()
            || self.diagnostic_error_color.is_some()
            || self.diagnostic_warning_color.is_some()
            || self.diagnostic_info_color.is_some()
            || self.diagnostic_hint_color.is_some()
            || self.syntax_color_fn.is_some()
    }

    /// Content-safe editor state summary for diagnostics and agent inspection.
    pub fn to_text(&self) -> String {
        let selection_summary = self
            .selection
            .as_ref()
            .map(|selection| selection.to_text())
            .unwrap_or_else(|| "none".to_string());

        format!(
            "editor_state(language={}, syntax_backend={}, lines={}, content_len_bytes={}, cursor={}, selection={}, has_selection={}, selection_empty={}, modified={}, has_file_path={}, undo_depth={}, redo_depth={}, has_syntax_tree={}, has_highlight_query={}, show_line_numbers={}, tab_size={}, read_only={}, search_query_len_bytes={}, has_search_query={}, search_match_count={}, has_current_match={}, search_case_sensitive={}, search_use_regex={}, fold_range_count={}, folded_range_count={}, diagnostics={}, errors={}, warnings={}, information={}, hints={}, has_visual_overrides={})",
            self.language.to_text(),
            self.syntax_backend().to_text(),
            self.line_count(),
            self.content_len_bytes(),
            self.cursor.to_text(),
            selection_summary,
            self.has_selection(),
            self.selection_is_empty(),
            self.is_modified,
            self.has_file_path(),
            self.undo_depth(),
            self.redo_depth(),
            self.has_syntax_tree(),
            self.has_highlight_query(),
            self.show_line_numbers,
            self.tab_size(),
            self.is_read_only(),
            self.search_query_len_bytes(),
            self.has_search_query(),
            self.search_match_count(),
            self.has_current_match(),
            self.search_case_sensitive(),
            self.search_use_regex(),
            self.fold_range_count(),
            self.folded_range_count(),
            self.diagnostic_count(),
            self.diagnostic_count_by_severity(DiagnosticSeverity::Error),
            self.diagnostic_count_by_severity(DiagnosticSeverity::Warning),
            self.diagnostic_count_by_severity(DiagnosticSeverity::Information),
            self.diagnostic_count_by_severity(DiagnosticSeverity::Hint),
            self.has_visual_overrides()
        )
    }

    pub fn content(&self) -> String {
        self.rope.to_string()
    }

    pub fn is_empty(&self) -> bool {
        self.rope.len_bytes() == 0 || (self.rope.len_bytes() == 1 && self.rope.len_lines() <= 1)
    }

    pub fn line_count(&self) -> usize {
        let lines = self.rope.len_lines();
        if lines > 0 && self.rope.len_bytes() > 0 {
            let last_line = self.rope.line(lines - 1);
            if last_line.len_bytes() == 0 {
                return lines.saturating_sub(1).max(1);
            }
        }
        lines.max(1)
    }

    pub fn cursor(&self) -> Position {
        self.cursor
    }

    pub fn is_modified(&self) -> bool {
        self.is_modified
    }

    pub fn file_path(&self) -> Option<&PathBuf> {
        self.file_path.as_ref()
    }

    pub fn language(&self) -> Language {
        self.language
    }

    /// Syntax-processing backend selected for this editor target.
    pub const fn syntax_backend(&self) -> EditorSyntaxBackend {
        EditorSyntaxBackend::current()
    }

    pub fn syntax_tree(&self) -> Option<&Tree> {
        self.syntax_tree.as_ref()
    }

    pub fn word_at_cursor(&self) -> Option<(String, usize)> {
        let line_text = self.line_text(self.cursor.line);
        if line_text.is_empty() || self.cursor.col == 0 {
            return None;
        }

        let bytes = line_text.as_bytes();
        let col = self.cursor.col.min(bytes.len());

        let mut word_start = col;
        while word_start > 0 {
            let ch = bytes[word_start - 1];
            if !ch.is_ascii_alphanumeric() && ch != b'_' {
                break;
            }
            word_start -= 1;
        }

        if word_start == col {
            return None;
        }

        let word = line_text[word_start..col].to_string();
        Some((word, word_start))
    }

    pub fn find_matching_bracket(&self) -> Option<(Position, Position)> {
        let line_text = self.line_text(self.cursor.line);
        let col = self.cursor.col.min(line_text.len());
        let bytes = line_text.as_bytes();

        let check_positions: &[usize] = if col > 0 { &[col, col - 1] } else { &[col] };

        for &check_col in check_positions {
            if check_col >= bytes.len() {
                continue;
            }
            let ch = bytes[check_col] as char;
            let (opener, closer, forward) = match ch {
                '(' => ('(', ')', true),
                '[' => ('[', ']', true),
                '{' => ('{', '}', true),
                ')' => ('(', ')', false),
                ']' => ('[', ']', false),
                '}' => ('{', '}', false),
                _ => continue,
            };

            let start_pos = Position::new(self.cursor.line, check_col);

            if forward {
                let mut depth = 1i32;
                let mut scan_line = self.cursor.line;
                let mut scan_col = check_col + 1;
                let total = self.total_lines();
                while scan_line < total {
                    let scan_text = self.line_text(scan_line);
                    let scan_bytes = scan_text.as_bytes();
                    while scan_col < scan_bytes.len() {
                        let sc = scan_bytes[scan_col] as char;
                        if sc == opener {
                            depth += 1;
                        } else if sc == closer {
                            depth -= 1;
                            if depth == 0 {
                                return Some((start_pos, Position::new(scan_line, scan_col)));
                            }
                        }
                        scan_col += 1;
                    }
                    scan_line += 1;
                    scan_col = 0;
                }
            } else {
                let mut depth = 1i32;
                let mut scan_line = self.cursor.line;
                let mut scan_col = check_col as i64 - 1;
                loop {
                    if scan_col < 0 {
                        if scan_line == 0 {
                            break;
                        }
                        scan_line -= 1;
                        let prev_text = self.line_text(scan_line);
                        scan_col = prev_text.len() as i64 - 1;
                        continue;
                    }
                    let scan_text = self.line_text(scan_line);
                    let scan_bytes = scan_text.as_bytes();
                    if (scan_col as usize) < scan_bytes.len() {
                        let sc = scan_bytes[scan_col as usize] as char;
                        if sc == closer {
                            depth += 1;
                        } else if sc == opener {
                            depth -= 1;
                            if depth == 0 {
                                return Some((
                                    Position::new(scan_line, scan_col as usize),
                                    start_pos,
                                ));
                            }
                        }
                    }
                    scan_col -= 1;
                }
            }
        }
        None
    }

    pub fn word_under_cursor_full(&self) -> Option<(String, usize, usize)> {
        let line_text = self.line_text(self.cursor.line);
        if line_text.is_empty() {
            return None;
        }
        let bytes = line_text.as_bytes();
        let col = self.cursor.col.min(bytes.len());

        let mut word_start = col;
        while word_start > 0
            && (bytes[word_start - 1].is_ascii_alphanumeric() || bytes[word_start - 1] == b'_')
        {
            word_start -= 1;
        }

        let mut word_end = col;
        while word_end < bytes.len()
            && (bytes[word_end].is_ascii_alphanumeric() || bytes[word_end] == b'_')
        {
            word_end += 1;
        }

        if word_start == word_end {
            return None;
        }

        let word = line_text[word_start..word_end].to_string();
        if word.len() < 2 {
            return None;
        }
        Some((word, word_start, word_end))
    }

    pub fn compute_fold_ranges(&mut self) {
        let ranges = self
            .syntax_tree
            .as_ref()
            .map(Self::collect_fold_ranges)
            .unwrap_or_default();
        self.install_fold_ranges(ranges);
    }

    fn collect_fold_ranges(tree: &Tree) -> Vec<FoldRange> {
        let mut ranges = Vec::new();
        let mut tree_cursor = tree.root_node().walk();
        let mut did_enter = true;

        loop {
            let node = tree_cursor.node();
            if did_enter {
                let kind = node.kind();
                let start_line = node.start_position().row;
                let end_line = node.end_position().row;
                if end_line > start_line + 1 && Self::is_foldable_kind(kind) {
                    ranges.push(FoldRange {
                        start_line,
                        end_line,
                    });
                }
            }

            if (did_enter && tree_cursor.goto_first_child()) || tree_cursor.goto_next_sibling() {
                did_enter = true;
            } else if tree_cursor.goto_parent() {
                did_enter = false;
            } else {
                break;
            }
        }

        ranges.sort_by_key(|r| r.start_line);
        ranges.dedup_by_key(|r| r.start_line);
        ranges
    }

    fn install_fold_ranges(&mut self, ranges: Vec<FoldRange>) {
        self.fold_ranges = ranges;
        self.folded = self
            .folded
            .iter()
            .filter_map(|fold| {
                self.fold_ranges
                    .binary_search_by_key(&fold.start_line, |range| range.start_line)
                    .ok()
                    .map(|index| self.fold_ranges[index])
            })
            .collect();
        self.rebuild_fold_line_index();
    }

    fn is_foldable_kind(kind: &str) -> bool {
        matches!(
            kind,
            "function_item"
                | "impl_item"
                | "struct_item"
                | "enum_item"
                | "block"
                | "if_expression"
                | "match_expression"
                | "function_declaration"
                | "class_declaration"
                | "class_definition"
                | "method_definition"
                | "if_statement"
                | "for_statement"
                | "while_statement"
                | "for_expression"
                | "while_expression"
                | "object"
                | "array"
                | "trait_item"
                | "mod_item"
                | "use_declaration"
                | "const_item"
                | "static_item"
                | "macro_definition"
                | "interface_declaration"
                | "type_alias_declaration"
                | "arrow_function"
                | "function_expression"
                | "try_statement"
                | "switch_statement"
                | "match_block"
                | "closure_expression"
                | "dictionary"
                | "list"
                | "tuple"
        )
    }

    pub fn toggle_fold_at_line(&mut self, line: usize, cx: &mut Context<Self>) {
        if let Some(idx) = self.folded.iter().position(|f| f.start_line == line) {
            self.folded.remove(idx);
        } else if let Some(range) = self.fold_ranges.iter().find(|r| r.start_line == line) {
            self.folded.push(*range);
        }
        self.invalidate_folds();
        self.clamp_scroll_after_fold();
        cx.notify();
    }

    pub fn fold_all(&mut self, cx: &mut Context<Self>) {
        self.folded = self.fold_ranges.clone();
        self.invalidate_folds();
        self.clamp_scroll_after_fold();
        cx.notify();
    }

    pub fn unfold_all(&mut self, cx: &mut Context<Self>) {
        self.folded.clear();
        self.invalidate_folds();
        self.clamp_scroll_after_fold();
        cx.notify();
    }

    fn invalidate_folds(&mut self) {
        self.invalidate_all_caches();
        self.rebuild_fold_line_index();
    }

    fn rebuild_fold_line_index(&mut self) {
        self.fold_layout_revision = self.fold_layout_revision.wrapping_add(1);
        self.fold_line_index = if self.folded.is_empty() {
            None
        } else {
            Some(Arc::new(FoldLineIndex::new(
                self.total_lines(),
                &self.folded,
            )))
        };
    }

    fn display_line_index(&self) -> DisplayLineIndex {
        let eof = self.reveal_eof
            || self.cursor.line >= self.total_lines()
            || self.selection.is_some_and(|selection| {
                selection.anchor.line >= self.total_lines()
                    || selection.cursor.line >= self.total_lines()
            });
        if self.folded.is_empty() {
            DisplayLineIndex::Unfolded(
                self.total_lines()
                    + usize::from(
                        eof && self.rope.line(self.rope.len_lines() - 1).len_bytes() == 0,
                    ),
            )
        } else {
            let index = self
                .fold_line_index
                .clone()
                .expect("fold mutations prepare their retained interval index");
            if eof {
                DisplayLineIndex::FoldedWithEof(index)
            } else {
                DisplayLineIndex::Folded(index)
            }
        }
    }

    fn clamp_scroll_after_fold(&mut self) {
        let line_height = self.line_height;
        let padding_top = px(12.0);
        let padding_bottom = px(12.0);
        let display_count = self.display_line_count();
        let content_height = padding_top + padding_bottom + line_height * display_count as f32;
        let viewport_height = self.scroll_handle.bounds().size.height;

        if viewport_height <= px(0.0) {
            return;
        }

        let max_scroll = (content_height - viewport_height).max(px(0.0));
        let offset = self.scroll_handle.offset();
        let clamped_y = offset.y.max(-max_scroll).min(px(0.0));
        if (clamped_y - offset.y).abs() > px(0.1) {
            self.scroll_handle.set_offset(point(offset.x, clamped_y));
        }
    }

    pub fn is_line_folded(&self, line: usize) -> bool {
        line < self.total_lines() && self.display_line_index().row_for_line(line).is_none()
    }

    /// Enumerate all displayed buffer lines. This explicit compatibility API
    /// allocates in proportion to the document; rendering/navigation instead
    /// use the retained interval index and enumerate only their viewport.
    pub fn display_lines(&self) -> Rc<Vec<usize>> {
        let index = self.display_line_index();
        Rc::new(index.visible_range(0..index.len()))
    }

    pub fn display_line_count(&self) -> usize {
        self.display_line_index().len()
    }

    pub fn buffer_line_to_display_row(&self, buffer_line: usize) -> Option<usize> {
        self.display_line_index().row_for_line(buffer_line)
    }

    pub fn display_row_to_buffer_line(&self, display_row: usize) -> usize {
        self.display_line_index()
            .line_for_row(display_row)
            .unwrap_or_else(|| self.total_lines().saturating_sub(1))
    }

    pub fn fold_ranges(&self) -> &[FoldRange] {
        &self.fold_ranges
    }

    pub fn folded_ranges(&self) -> &[FoldRange] {
        &self.folded
    }

    pub fn scope_breadcrumbs(&self) -> Vec<(String, usize)> {
        let tree = match &self.syntax_tree {
            Some(t) => t,
            None => return Vec::new(),
        };

        let byte_offset = self.pos_to_byte_offset(self.cursor);
        let ts_point = self.byte_to_ts_point(byte_offset);
        let mut node = match tree
            .root_node()
            .descendant_for_point_range(ts_point, ts_point)
        {
            Some(n) => n,
            None => return Vec::new(),
        };

        let mut breadcrumbs = Vec::new();
        loop {
            let kind = node.kind();
            if Self::is_scope_kind(kind)
                && let Some(name) = Self::extract_scope_name(&node, &self.rope)
            {
                let line = node.start_position().row;
                breadcrumbs.push((name, line));
            }
            match node.parent() {
                Some(p) => node = p,
                None => break,
            }
        }
        breadcrumbs.reverse();
        breadcrumbs
    }

    fn is_scope_kind(kind: &str) -> bool {
        matches!(
            kind,
            "function_item"
                | "impl_item"
                | "struct_item"
                | "enum_item"
                | "trait_item"
                | "mod_item"
                | "function_declaration"
                | "class_declaration"
                | "class_definition"
                | "method_definition"
                | "interface_declaration"
                | "module"
                | "namespace_definition"
        )
    }

    fn extract_scope_name(node: &tree_sitter::Node, rope: &Rope) -> Option<String> {
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                let kind = child.kind();
                if kind == "name"
                    || kind == "identifier"
                    || kind == "type_identifier"
                    || kind == "property_identifier"
                {
                    let start = child.start_byte();
                    let end = child.end_byte().min(rope.len_bytes());
                    if start < end {
                        let name: String = rope.byte_slice(start..end).into();
                        return Some(name);
                    }
                }
            }
        }
        None
    }

    fn closing_char_for(&self, ch: char) -> Option<char> {
        for &(opener, closer) in AUTO_CLOSE_PAIRS {
            if ch == opener {
                if opener == closer {
                    let line_text = self.line_text(self.cursor.line);
                    let col = self.cursor.col.min(line_text.len());
                    let before = &line_text[..col];
                    let count = before.chars().filter(|&c| c == ch).count();
                    if count % 2 != 0 {
                        return None;
                    }
                }
                return Some(closer);
            }
        }
        None
    }

    fn should_skip_closing_char(&self, ch: char) -> bool {
        let is_closer = AUTO_CLOSE_PAIRS.iter().any(|&(_, c)| c == ch);
        if !is_closer {
            return false;
        }
        let line_text = self.line_text(self.cursor.line);
        let col = self.cursor.col;
        if col < line_text.len() {
            let next_ch = line_text[col..].chars().next();
            return next_ch == Some(ch);
        }
        false
    }

    fn is_between_auto_close_pair(&self) -> bool {
        let line_text = self.line_text(self.cursor.line);
        let col = self.cursor.col;
        if col == 0 || col >= line_text.len() {
            return false;
        }
        let before = line_text.as_bytes()[col - 1];
        let after = line_text.as_bytes()[col];
        AUTO_CLOSE_PAIRS
            .iter()
            .any(|&(o, c)| before == o as u8 && after == c as u8)
    }

    pub fn cursor_screen_position(&self, line_height: Pixels) -> Option<Point<Pixels>> {
        let bounds = self.last_bounds?;
        let gutter_width = if self.show_line_numbers {
            px(80.0)
        } else {
            px(12.0)
        };
        let padding_top = px(12.0);

        let cursor_y = bounds.top() + padding_top + line_height * (self.cursor.line as f32);

        let cursor_x = if let Some(layout) = self.line_layouts.get(&self.cursor.line) {
            let line_text = self.line_text(self.cursor.line);
            let char_offset = self.cursor.col.min(line_text.len());
            let x_offset = layout.x_for_index(char_offset);
            bounds.left() + gutter_width + x_offset
        } else {
            let approx_char_width = px(8.4);
            bounds.left() + gutter_width + approx_char_width * (self.cursor.col as f32)
        };

        Some(Point::new(cursor_x, cursor_y + line_height))
    }

    pub fn apply_completion(
        &mut self,
        trigger_col: usize,
        insert_text: &str,
        cx: &mut Context<Self>,
    ) {
        if self.read_only || self.disabled {
            return;
        }

        let delete_count = self.cursor.col.saturating_sub(trigger_col);
        if delete_count > 0 {
            let start_pos = Position::new(self.cursor.line, trigger_col);
            let end_pos = self.cursor;
            self.delete_selection_internal(Selection::new(start_pos, end_pos), cx);
        }

        self.insert_text_at_cursor(insert_text, cx);
        self.ensure_cursor_visible(cx);
        cx.notify();
    }

    fn line_text(&self, line: usize) -> String {
        if line >= self.rope.len_lines() {
            return String::new();
        }
        let line_slice = self.rope.line(line);
        let mut s = line_slice.to_string();
        if s.ends_with('\n') {
            s.pop();
        }
        if s.ends_with('\r') {
            s.pop();
        }
        s
    }

    fn line_len(&self, line: usize) -> usize {
        self.line_text(line).len()
    }

    fn previous_grapheme_column(&self, position: Position) -> usize {
        self.line_text(position.line)
            .grapheme_indices(true)
            .map(|(offset, _)| offset)
            .take_while(|offset| *offset < position.col)
            .last()
            .unwrap_or(0)
    }

    fn next_grapheme_column(&self, position: Position) -> usize {
        let text = self.line_text(position.line);
        text.grapheme_indices(true)
            .map(|(offset, _)| offset)
            .find(|offset| *offset > position.col)
            .unwrap_or(text.len())
    }

    fn rope_insert(&mut self, byte_offset: usize, text: &str) {
        let char_offset = self
            .rope
            .byte_to_char(byte_offset.min(self.rope.len_bytes()));
        self.rope.insert(char_offset, text);
    }

    fn rope_remove(&mut self, byte_start: usize, byte_end: usize) {
        let len = self.rope.len_bytes();
        let char_start = self.rope.byte_to_char(byte_start.min(len));
        let char_end = self.rope.byte_to_char(byte_end.min(len));
        self.rope.remove(char_start..char_end);
    }

    fn total_lines(&self) -> usize {
        self.line_count()
    }

    pub fn set_content(&mut self, content: &str, cx: &mut Context<Self>) {
        self.reveal_eof = false;
        self.accessibility_reveal = None;
        self.accessibility_geometry = None;
        self.reparse_task = None;
        self.content_version = self.content_version.wrapping_add(1);
        self.rope = if content.is_empty() {
            Rope::from_str("\n")
        } else if content.ends_with('\n') {
            Rope::from_str(content)
        } else {
            let mut s = content.to_string();
            s.push('\n');
            Rope::from_str(&s)
        };
        self.cursor = Position::zero();
        self.selection = None;
        self.marked_range = None;
        self.folded.clear();
        self.fold_ranges.clear();
        self.fold_line_index = None;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.is_modified = false;
        self.invalidate_all_caches();
        if self.rope.len_bytes() > 50_000 {
            self.parse_async(cx);
        } else {
            self.update_syntax_tree();
        }
        cx.notify();
    }

    pub fn set_language(&mut self, lang: Language) {
        self.reparse_task = None;
        self.language = lang;
        let tree_sitter_language = lang.tree_sitter_language();
        if let Some(ts_lang) = tree_sitter_language {
            let _ = self.parser.set_language(&ts_lang);
            self.highlight_query = lang
                .highlight_query_source()
                .filter(|src| !src.is_empty())
                .and_then(|src| Query::new(&ts_lang, &src).ok());
        } else {
            self.highlight_query = None;
        }
        self.update_syntax_tree();
    }

    pub fn set_overlay_active_check(&mut self, check: impl Fn(&App) -> bool + 'static) {
        self.overlay_active_check = Some(Box::new(check));
    }

    fn is_overlay_active(&self, cx: &App) -> bool {
        self.overlay_active_check
            .as_ref()
            .map(|check| check(cx))
            .unwrap_or(false)
    }

    pub fn load_file(&mut self, path: impl Into<PathBuf>, cx: &mut Context<Self>) {
        let path = path.into();
        let lang = Language::from_path(&path);
        self.language = lang;
        let tree_sitter_language = lang.tree_sitter_language();
        if let Some(ts_lang) = tree_sitter_language {
            let _ = self.parser.set_language(&ts_lang);
            self.highlight_query = lang
                .highlight_query_source()
                .filter(|src| !src.is_empty())
                .and_then(|src| Query::new(&ts_lang, &src).ok());
        } else {
            self.highlight_query = None;
        }

        match std::fs::File::open(&path) {
            Ok(file) => {
                let reader = std::io::BufReader::new(file);
                match Rope::from_reader(reader) {
                    Ok(rope) => {
                        self.file_path = Some(path);
                        self.rope = rope;
                        self.content_version = self.content_version.wrapping_add(1);
                        self.cursor = Position::zero();
                        self.selection = None;
                        self.marked_range = None;
                        self.undo_stack.clear();
                        self.redo_stack.clear();
                        self.is_modified = false;
                        self.invalidate_all_caches();
                        if self.rope.len_bytes() > 50_000 {
                            self.parse_async(cx);
                        } else {
                            self.update_syntax_tree();
                        }
                        cx.notify();
                    }
                    Err(_) => {
                        self.file_path = Some(path);
                        self.set_content("", cx);
                        self.is_modified = false;
                    }
                }
            }
            Err(_) => {
                self.file_path = Some(path);
                self.set_content("", cx);
                self.is_modified = false;
            }
        }
    }

    pub fn save_to_file(&mut self, path: impl Into<PathBuf>, cx: &mut Context<Self>) -> bool {
        let path = path.into();
        match std::fs::File::create(&path) {
            Ok(file) => {
                let mut writer = std::io::BufWriter::new(file);
                match self.rope.write_to(&mut writer) {
                    Ok(()) => {
                        self.file_path = Some(path);
                        self.is_modified = false;
                        cx.notify();
                        true
                    }
                    Err(_) => false,
                }
            }
            Err(_) => false,
        }
    }

    pub fn save(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(path) = self.file_path.clone() {
            self.save_to_file(path, cx)
        } else {
            false
        }
    }

    fn update_syntax_tree(&mut self) {
        let rope = &self.rope;
        self.syntax_tree = self.parser.parse_with_options(
            &mut |byte_idx, _pos| -> &[u8] {
                if byte_idx >= rope.len_bytes() {
                    return &[];
                }
                let (chunk, start, _, _) = rope.chunk_at_byte(byte_idx);
                &chunk.as_bytes()[byte_idx - start..]
            },
            None,
            None,
        );
    }

    fn byte_to_ts_point(&self, byte_offset: usize) -> TSPoint {
        let line = self.rope.byte_to_line(byte_offset);
        let line_start = self.rope.line_to_byte(line);
        TSPoint::new(line, byte_offset - line_start)
    }

    fn update_syntax_tree_incremental(
        &mut self,
        start_byte: usize,
        old_end_byte: usize,
        new_end_byte: usize,
        old_end_position: TSPoint,
        cx: &mut Context<Self>,
    ) {
        let start_position = self.byte_to_ts_point(start_byte);
        let new_end_position = self.byte_to_ts_point(new_end_byte.min(self.rope.len_bytes()));
        if let Some(tree) = &mut self.syntax_tree {
            tree.edit(&InputEdit {
                start_byte,
                old_end_byte,
                new_end_byte,
                start_position,
                old_end_position,
                new_end_position,
            });
        }
        self.schedule_reparse(cx);
    }

    fn parse_async(&mut self, cx: &mut Context<Self>) {
        self.syntax_tree = None;
        self.reparse_task = None;
        let lang = self.language;
        let Some(tree_sitter_language) = lang.tree_sitter_language() else {
            self.install_fold_ranges(Vec::new());
            return;
        };
        let revision = self.content_version;
        let rope = self.rope.clone();
        // Rope clones share storage; flattening text, parsing, and walking the
        // syntax tree for available folds all belong on the worker.
        let parse_task = cx.background_spawn(async move {
            let mut parser = Parser::new();
            let _ = parser.set_language(&tree_sitter_language);
            let content = rope.to_string();
            let tree = parser.parse(&content, None);
            let ranges = tree
                .as_ref()
                .map(Self::collect_fold_ranges)
                .unwrap_or_default();
            (tree, ranges)
        });
        self.reparse_task = Some(cx.spawn(async move |this, cx| {
            let (tree, ranges) = parse_task.await;
            let _ = this.update(cx, |state, cx| {
                if state.content_version != revision || state.language != lang {
                    return;
                }
                state.reparse_task = None;
                state.syntax_tree = tree;
                state.install_fold_ranges(ranges);
                state.invalidate_all_caches();
                cx.notify();
            });
        }));
    }

    fn schedule_reparse(&mut self, cx: &mut Context<Self>) {
        let entity = cx.entity().clone();
        self.reparse_task = Some(cx.spawn(async move |_, cx| {
            Timer::after(Duration::from_millis(50)).await;
            let _ = cx.update(|cx| {
                entity.update(cx, |state, cx| {
                    state.update_syntax_tree_incremental_now();
                    state.compute_fold_ranges();
                    // Only invalidate highlights — line layouts are still valid
                    // since text content hasn't changed (only syntax tree updated).
                    state.invalidate_after_edit();
                    cx.notify();
                });
            });
        }));
    }

    fn update_syntax_tree_incremental_now(&mut self) {
        if self.syntax_tree.is_none() {
            return;
        }
        let rope = &self.rope;
        self.syntax_tree = self.parser.parse_with_options(
            &mut |byte_idx, _pos| -> &[u8] {
                if byte_idx >= rope.len_bytes() {
                    return &[];
                }
                let (chunk, start, _, _) = rope.chunk_at_byte(byte_idx);
                &chunk.as_bytes()[byte_idx - start..]
            },
            self.syntax_tree.as_ref(),
            None,
        );
    }

    fn pos_to_byte_offset(&self, pos: Position) -> usize {
        if pos.line >= self.rope.len_lines() {
            return self.rope.len_bytes();
        }
        let line_start = self.rope.line_to_byte(pos.line);
        let text = self.line_text(pos.line);
        let mut column = min(pos.col, text.len());
        while !text.is_char_boundary(column) {
            column -= 1;
        }
        line_start + column
    }

    fn byte_offset_to_pos(&self, offset: usize) -> Position {
        let offset = min(offset, self.rope.len_bytes());
        let line = self.rope.byte_to_line(offset);
        let line_start = self.rope.line_to_byte(line);
        let col = offset - line_start;
        Position::new(line, col)
    }

    fn clamp_cursor(&mut self) {
        let max_line = self.total_lines().saturating_sub(1);
        self.cursor.line = min(self.cursor.line, max_line);
        self.cursor = self.byte_offset_to_pos(self.pos_to_byte_offset(self.cursor));
    }

    fn mark_modified(&mut self) {
        self.is_modified = true;
        self.content_version = self.content_version.wrapping_add(1);
        self.cursor_visible = true;
        self.last_cursor_move = web_time::Instant::now();
    }

    pub fn content_version(&self) -> u64 {
        self.content_version
    }

    fn insert_text_at_cursor(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.read_only || self.disabled {
            return;
        }
        if let Some(selection) = self.selection.take() {
            self.delete_selection_internal(selection, cx);
        }

        let byte_offset = self.pos_to_byte_offset(self.cursor);
        let old_end_position = self.byte_to_ts_point(byte_offset);
        self.undo_stack.push(EditOp::Insert {
            byte_offset,
            text: text.to_string(),
        });
        self.redo_stack.clear();

        self.rope_insert(byte_offset, text);
        self.mark_modified();

        let new_end_byte = byte_offset + text.len();
        self.cursor = self.byte_offset_to_pos(new_end_byte);
        self.update_syntax_tree_incremental(
            byte_offset,
            byte_offset,
            new_end_byte,
            old_end_position,
            cx,
        );
        self.invalidate_after_edit();
    }

    fn delete_selection_internal(&mut self, selection: Selection, cx: &mut Context<Self>) {
        let (start, end) = selection.range();
        let start_offset = self.pos_to_byte_offset(start);
        let end_offset = self.pos_to_byte_offset(end);

        if start_offset >= end_offset {
            self.cursor = start;
            return;
        }

        let old_end_position = self.byte_to_ts_point(end_offset);
        let deleted: String = self.rope.byte_slice(start_offset..end_offset).into();
        self.undo_stack.push(EditOp::Delete {
            byte_offset: start_offset,
            text: deleted,
        });
        self.redo_stack.clear();

        self.rope_remove(start_offset, end_offset);
        self.mark_modified();
        self.cursor = start;
        self.clamp_cursor();
        self.update_syntax_tree_incremental(
            start_offset,
            end_offset,
            start_offset,
            old_end_position,
            cx,
        );
        self.invalidate_after_edit();
    }

    fn get_selection_text(&self, selection: &Selection) -> String {
        let (start, end) = selection.range();
        let start_offset = self.pos_to_byte_offset(start);
        let end_offset = self.pos_to_byte_offset(end);
        if start_offset >= end_offset {
            return String::new();
        }
        self.rope.byte_slice(start_offset..end_offset).into()
    }

    fn find_word_boundary_left(&self, pos: Position) -> Position {
        if pos.col == 0 {
            if pos.line == 0 {
                return pos;
            }
            return Position::new(pos.line - 1, self.line_len(pos.line - 1));
        }
        let line_text = self.line_text(pos.line);
        let col = self.pos_to_byte_offset(pos) - self.rope.line_to_byte(pos.line);
        let mut graphemes = line_text[..col]
            .grapheme_indices(true)
            .rev()
            .skip_while(|(_, text)| text.chars().all(char::is_whitespace));
        let Some((mut col, text)) = graphemes.next() else {
            return Position::new(pos.line, 0);
        };
        if text
            .chars()
            .next()
            .is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
        {
            for (offset, text) in graphemes {
                if !text
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
                {
                    break;
                }
                col = offset;
            }
        }
        Position::new(pos.line, col)
    }

    fn find_word_boundary_right(&self, pos: Position) -> Position {
        let line_len = self.line_len(pos.line);
        if pos.col >= line_len {
            if pos.line >= self.total_lines() - 1 {
                return pos;
            }
            return Position::new(pos.line + 1, 0);
        }
        let line_text = self.line_text(pos.line);
        let start = self.pos_to_byte_offset(pos) - self.rope.line_to_byte(pos.line);
        let mut graphemes = line_text[start..].graphemes(true).peekable();
        let mut col = start;
        if let Some(text) = graphemes.next() {
            col += text.len();
            if text
                .chars()
                .next()
                .is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
            {
                while let Some(text) = graphemes.peek() {
                    if !text
                        .chars()
                        .next()
                        .is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
                    {
                        break;
                    }
                    col += text.len();
                    graphemes.next();
                }
            }
        }
        while let Some(text) = graphemes.peek() {
            if !text.chars().all(char::is_whitespace) {
                break;
            }
            col += text.len();
            graphemes.next();
        }
        Position::new(pos.line, min(col, line_len))
    }

    // UTF-16 conversion helpers for IME support
    fn offset_to_utf16(&self, byte_offset: usize) -> usize {
        let byte_offset = min(byte_offset, self.rope.len_bytes());
        let char_offset = self.rope.byte_to_char(byte_offset);
        self.rope.char_to_utf16_cu(char_offset)
    }

    fn offset_from_utf16(&self, utf16_offset: usize) -> usize {
        let utf16_offset = utf16_offset.min(self.rope.len_utf16_cu());
        let mut char_offset = self.rope.utf16_cu_to_char(utf16_offset);
        // Preserve the input handler's existing forward adjustment when an
        // editing range ends inside a surrogate pair.
        if self.rope.char_to_utf16_cu(char_offset) < utf16_offset {
            char_offset += 1;
        }
        self.rope.char_to_byte(char_offset)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    fn input_replacement_range(&self) -> Range<usize> {
        self.marked_range.clone().unwrap_or_else(|| {
            if let Some(selection) = &self.selection {
                let (start, end) = selection.range();
                self.pos_to_byte_offset(start)..self.pos_to_byte_offset(end)
            } else {
                let offset = self.pos_to_byte_offset(self.cursor);
                offset..offset
            }
        })
    }

    fn replace_input_range(
        &mut self,
        range: Range<usize>,
        text: &str,
        coalesce_marked: bool,
        cx: &mut Context<Self>,
    ) {
        let before: String = self.rope.byte_slice(range.clone()).into();
        self.selection = None;
        if before == text {
            self.cursor = self.byte_offset_to_pos(range.start + text.len());
            return;
        }
        let old_end_position = self.byte_to_ts_point(range.end);
        let coalesced = coalesce_marked
            && self.marked_range.as_ref() == Some(&range)
            && if let Some(EditOp::Replace {
                byte_offset, after, ..
            }) = self.undo_stack.last_mut()
            {
                if *byte_offset == range.start && *after == before {
                    *after = text.to_string();
                    true
                } else {
                    false
                }
            } else {
                false
            };
        if !coalesced {
            self.undo_stack.push(EditOp::Replace {
                byte_offset: range.start,
                before,
                after: text.to_string(),
            });
        }
        self.redo_stack.clear();
        self.rope_remove(range.start, range.end);
        self.rope_insert(range.start, text);
        let new_end = range.start + text.len();
        self.cursor = self.byte_offset_to_pos(new_end);
        self.mark_modified();
        self.update_syntax_tree_incremental(range.start, range.end, new_end, old_end_position, cx);
        self.invalidate_after_edit();
    }

    /// Replace the current selection as one undoable edit. This is useful for
    /// document formatting commands and does not reload/reset the document.
    pub fn replace_selection(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.read_only || self.disabled {
            return;
        }
        let range = self.input_replacement_range();
        self.replace_input_range(range, text, false, cx);
        self.marked_range = None;
        self.ensure_cursor_visible(cx);
        cx.notify();
    }

    pub fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only || self.disabled {
            return;
        }
        self.marked_range = None;
        if let Some(op) = self.undo_stack.pop() {
            match &op {
                EditOp::Insert { byte_offset, text } => {
                    let end = byte_offset + text.len();
                    self.rope_remove(*byte_offset, end);
                    self.cursor = self.byte_offset_to_pos(*byte_offset);
                    self.redo_stack.push(op);
                }
                EditOp::Delete { byte_offset, text } => {
                    self.rope_insert(*byte_offset, text);
                    self.cursor = self.byte_offset_to_pos(*byte_offset + text.len());
                    self.redo_stack.push(op);
                }
                EditOp::Replace {
                    byte_offset,
                    before,
                    after,
                } => {
                    self.rope_remove(*byte_offset, *byte_offset + after.len());
                    self.rope_insert(*byte_offset, before);
                    self.cursor = self.byte_offset_to_pos(*byte_offset + before.len());
                    self.redo_stack.push(op);
                }
            }
            self.selection = None;
            self.mark_modified();
            self.update_syntax_tree();
            self.invalidate_after_edit();
            cx.notify();
        }
    }

    pub fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only || self.disabled {
            return;
        }
        self.marked_range = None;
        if let Some(op) = self.redo_stack.pop() {
            match &op {
                EditOp::Insert { byte_offset, text } => {
                    self.rope_insert(*byte_offset, text);
                    self.cursor = self.byte_offset_to_pos(*byte_offset + text.len());
                    self.undo_stack.push(op);
                }
                EditOp::Delete { byte_offset, text } => {
                    let end = byte_offset + text.len();
                    self.rope_remove(*byte_offset, end);
                    self.cursor = self.byte_offset_to_pos(*byte_offset);
                    self.undo_stack.push(op);
                }
                EditOp::Replace {
                    byte_offset,
                    before,
                    after,
                } => {
                    self.rope_remove(*byte_offset, *byte_offset + before.len());
                    self.rope_insert(*byte_offset, after);
                    self.cursor = self.byte_offset_to_pos(*byte_offset + after.len());
                    self.undo_stack.push(op);
                }
            }
            self.selection = None;
            self.mark_modified();
            self.update_syntax_tree();
            self.invalidate_after_edit();
            cx.notify();
        }
    }

    pub fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.is_overlay_active(cx) {
            cx.propagate();
            return;
        }
        if self.cursor.line > 0 {
            self.cursor.line -= 1;
            self.clamp_cursor();
        }
        self.selection = None;
        cx.notify();
    }

    pub fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.is_overlay_active(cx) {
            cx.propagate();
            return;
        }
        if self.cursor.line < self.total_lines() - 1 {
            self.cursor.line += 1;
            self.clamp_cursor();
        }
        self.selection = None;
        cx.notify();
    }

    pub fn move_left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.cursor.col > 0 {
            self.cursor.col = self.previous_grapheme_column(self.cursor);
        } else if self.cursor.line > 0 {
            self.cursor.line -= 1;
            self.cursor.col = self.line_len(self.cursor.line);
        }
        self.selection = None;
        cx.notify();
    }

    pub fn move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let line_len = self.line_len(self.cursor.line);
        if self.cursor.col < line_len {
            self.cursor.col = self.next_grapheme_column(self.cursor);
        } else if self.cursor.line < self.total_lines() - 1 {
            self.cursor.line += 1;
            self.cursor.col = 0;
        }
        self.selection = None;
        cx.notify();
    }

    pub fn move_word_left(&mut self, _: &MoveWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.cursor = self.find_word_boundary_left(self.cursor);
        self.selection = None;
        cx.notify();
    }

    pub fn move_word_right(&mut self, _: &MoveWordRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.cursor = self.find_word_boundary_right(self.cursor);
        self.selection = None;
        cx.notify();
    }

    pub fn move_to_line_start(
        &mut self,
        _: &MoveToLineStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        self.cursor.col = 0;
        self.selection = None;
        cx.notify();
    }

    pub fn move_to_line_end(&mut self, _: &MoveToLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.cursor.col = self.line_len(self.cursor.line);
        self.selection = None;
        cx.notify();
    }

    pub fn move_to_doc_start(
        &mut self,
        _: &MoveToDocStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        self.cursor = Position::zero();
        self.selection = None;
        cx.notify();
    }

    pub fn move_to_doc_end(&mut self, _: &MoveToDocEnd, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let last = self.total_lines() - 1;
        self.cursor = Position::new(last, self.line_len(last));
        self.selection = None;
        cx.notify();
    }

    pub fn page_up(&mut self, _: &PageUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let page_size = 30;
        self.cursor.line = self.cursor.line.saturating_sub(page_size);
        self.clamp_cursor();
        self.selection = None;
        cx.notify();
    }

    pub fn page_down(&mut self, _: &PageDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let page_size = 30;
        self.cursor.line = min(self.cursor.line + page_size, self.total_lines() - 1);
        self.clamp_cursor();
        self.selection = None;
        cx.notify();
    }

    fn start_selection_if_needed(&mut self) {
        if self.selection.is_none() {
            self.selection = Some(Selection::new(self.cursor, self.cursor));
        }
    }

    pub fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.start_selection_if_needed();
        if self.cursor.line > 0 {
            self.cursor.line -= 1;
            self.clamp_cursor();
            if let Some(ref mut sel) = self.selection {
                sel.cursor = self.cursor;
            }
            cx.notify();
        }
    }

    pub fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.start_selection_if_needed();
        if self.cursor.line < self.total_lines() - 1 {
            self.cursor.line += 1;
            self.clamp_cursor();
            if let Some(ref mut sel) = self.selection {
                sel.cursor = self.cursor;
            }
            cx.notify();
        }
    }

    pub fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.start_selection_if_needed();
        if self.cursor.col > 0 {
            self.cursor.col = self.previous_grapheme_column(self.cursor);
        } else if self.cursor.line > 0 {
            self.cursor.line -= 1;
            self.cursor.col = self.line_len(self.cursor.line);
        }
        if let Some(ref mut sel) = self.selection {
            sel.cursor = self.cursor;
        }
        cx.notify();
    }

    pub fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.start_selection_if_needed();
        let line_len = self.line_len(self.cursor.line);
        if self.cursor.col < line_len {
            self.cursor.col = self.next_grapheme_column(self.cursor);
        } else if self.cursor.line < self.total_lines() - 1 {
            self.cursor.line += 1;
            self.cursor.col = 0;
        }
        if let Some(ref mut sel) = self.selection {
            sel.cursor = self.cursor;
        }
        cx.notify();
    }

    pub fn select_to_line_start(
        &mut self,
        _: &SelectToLineStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        self.start_selection_if_needed();
        self.cursor.col = 0;
        if let Some(ref mut sel) = self.selection {
            sel.cursor = self.cursor;
        }
        cx.notify();
    }

    pub fn select_to_line_end(
        &mut self,
        _: &SelectToLineEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        self.start_selection_if_needed();
        self.cursor.col = self.line_len(self.cursor.line);
        if let Some(ref mut sel) = self.selection {
            sel.cursor = self.cursor;
        }
        cx.notify();
    }

    pub fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let start = Position::zero();
        let last = self.total_lines() - 1;
        let end = Position::new(last, self.line_len(last));
        self.selection = Some(Selection::new(start, end));
        self.cursor = end;
        cx.notify();
    }

    pub fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only || self.disabled {
            return;
        }
        if let Some(selection) = self
            .selection
            .take()
            .filter(|selection| !selection.is_empty())
        {
            self.delete_selection_internal(selection, cx);
            cx.notify();
            return;
        }
        let offset = self.pos_to_byte_offset(self.cursor);
        if offset == 0 {
            return;
        }

        let delete_pair = self.is_between_auto_close_pair();
        let char_idx = self.rope.byte_to_char(offset);
        let del_start = if self.cursor.col > 0 {
            self.rope.line_to_byte(self.cursor.line) + self.previous_grapheme_column(self.cursor)
        } else {
            self.rope.char_to_byte(char_idx.saturating_sub(1))
        };
        let del_end = if delete_pair {
            let next_char_byte = if char_idx < self.rope.len_chars() {
                self.rope.char_to_byte(char_idx + 1)
            } else {
                self.rope.len_bytes()
            };
            next_char_byte.min(self.rope.len_bytes())
        } else {
            offset
        };

        let old_end_position = self.byte_to_ts_point(del_end);
        let deleted: String = self.rope.byte_slice(del_start..del_end).into();
        self.undo_stack.push(EditOp::Delete {
            byte_offset: del_start,
            text: deleted,
        });
        self.redo_stack.clear();
        self.rope_remove(del_start, del_end);
        self.mark_modified();
        self.cursor = self.byte_offset_to_pos(del_start);
        self.update_syntax_tree_incremental(del_start, del_end, del_start, old_end_position, cx);
        self.invalidate_after_edit();
        cx.notify();
    }

    pub fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only || self.disabled {
            return;
        }
        if let Some(selection) = self
            .selection
            .take()
            .filter(|selection| !selection.is_empty())
        {
            self.delete_selection_internal(selection, cx);
            cx.notify();
            return;
        }
        let offset = self.pos_to_byte_offset(self.cursor);
        if offset >= self.rope.len_bytes() {
            return;
        }
        let char_idx = self.rope.byte_to_char(offset);
        let next_char_byte = if self.cursor.col < self.line_len(self.cursor.line) {
            self.rope.line_to_byte(self.cursor.line) + self.next_grapheme_column(self.cursor)
        } else if char_idx < self.rope.len_chars() {
            self.rope.char_to_byte(char_idx + 1)
        } else {
            self.rope.len_bytes()
        };
        let del_end = min(next_char_byte, self.rope.len_bytes());
        let old_end_position = self.byte_to_ts_point(del_end);
        let deleted: String = self.rope.byte_slice(offset..del_end).into();
        self.undo_stack.push(EditOp::Delete {
            byte_offset: offset,
            text: deleted,
        });
        self.redo_stack.clear();
        self.rope_remove(offset, del_end);
        self.mark_modified();
        self.update_syntax_tree_incremental(offset, del_end, offset, old_end_position, cx);
        self.invalidate_after_edit();
        cx.notify();
    }

    pub fn delete_word(&mut self, _: &DeleteWord, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only || self.disabled {
            return;
        }
        let word_start = self.find_word_boundary_left(self.cursor);
        if word_start == self.cursor {
            return;
        }
        let start_offset = self.pos_to_byte_offset(word_start);
        let end_offset = self.pos_to_byte_offset(self.cursor);
        let old_end_position = self.byte_to_ts_point(end_offset);
        let deleted: String = self.rope.byte_slice(start_offset..end_offset).into();
        self.undo_stack.push(EditOp::Delete {
            byte_offset: start_offset,
            text: deleted,
        });
        self.redo_stack.clear();
        self.rope_remove(start_offset, end_offset);
        self.mark_modified();
        self.cursor = word_start;
        self.update_syntax_tree_incremental(
            start_offset,
            end_offset,
            start_offset,
            old_end_position,
            cx,
        );
        self.invalidate_after_edit();
        cx.notify();
    }

    pub fn enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_overlay_active(cx) {
            cx.propagate();
            return;
        }
        if self.read_only || self.disabled {
            return;
        }

        let line_text = self.line_text(self.cursor.line);
        let before_cursor = &line_text[..self.cursor.col.min(line_text.len())];
        let after_cursor = &line_text[self.cursor.col.min(line_text.len())..];

        let base_indent = before_cursor.len() - before_cursor.trim_start().len();
        let trimmed = before_cursor.trim_end();
        let increase = matches!(trimmed.as_bytes().last(), Some(b'{' | b'(' | b'[' | b':'));

        let indent_str = " ".repeat(base_indent);
        let extra_indent = " ".repeat(self.tab_size);

        let after_trimmed = after_cursor.trim_start();
        let between_pair = increase
            && !after_trimmed.is_empty()
            && matches!(
                (trimmed.as_bytes().last(), after_trimmed.as_bytes().first()),
                (Some(b'{'), Some(b'}')) | (Some(b'('), Some(b')')) | (Some(b'['), Some(b']'))
            );

        if between_pair {
            let text = format!("\n{}{}\n{}", indent_str, extra_indent, indent_str);
            self.insert_text_at_cursor(&text, cx);
            let target_line = self.cursor.line - 1;
            let target_col = base_indent + self.tab_size;
            self.cursor = Position::new(target_line, target_col);
        } else if increase {
            let text = format!("\n{}{}", indent_str, extra_indent);
            self.insert_text_at_cursor(&text, cx);
        } else {
            let text = format!("\n{}", indent_str);
            self.insert_text_at_cursor(&text, cx);
        }
        self.ensure_cursor_visible(cx);
    }

    pub fn tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_overlay_active(cx) {
            cx.propagate();
            return;
        }
        if self.read_only || self.disabled {
            return;
        }
        let spaces = " ".repeat(self.tab_size);
        self.insert_text_at_cursor(&spaces, cx);
    }

    pub fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if let Some(selection) = &self.selection {
            let text = self.get_selection_text(selection);
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    pub fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only || self.disabled {
            return;
        }
        if let Some(selection) = self.selection.take() {
            let text = self.get_selection_text(&selection);
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.delete_selection_internal(selection, cx);
            cx.notify();
        }
    }

    pub fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only || self.disabled {
            return;
        }
        if let Ok(Some(item)) = cx.read_from_clipboard()
            && let Some(text) = item.text()
        {
            self.insert_text_at_cursor(&text, cx);
        }
    }

    pub fn selection_text(&self) -> Option<String> {
        self.selection
            .as_ref()
            .map(|sel| self.get_selection_text(sel))
            .filter(|s| !s.is_empty())
    }

    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    pub fn search_match_count(&self) -> usize {
        self.search_matches.len()
    }

    pub fn current_match_index(&self) -> Option<usize> {
        self.current_match_idx
    }

    pub fn search_case_sensitive(&self) -> bool {
        self.search_case_sensitive
    }

    pub fn search_use_regex(&self) -> bool {
        self.search_use_regex
    }

    pub fn find_all(&mut self, query: &str, cx: &mut Context<Self>) {
        self.search_query = query.to_string();

        if query.is_empty() {
            self.search_matches.clear();
            self.current_match_idx = None;
            self.search_task = None;
            cx.notify();
            return;
        }

        // Schedule a debounced search — any new call cancels the previous one.
        // This ensures typing always takes priority; search only runs after
        // the user stops typing for 200ms.
        self.schedule_search(cx);
    }

    fn schedule_search(&mut self, cx: &mut Context<Self>) {
        let query_owned = self.search_query.clone();
        let use_regex = self.search_use_regex;
        let case_sensitive = self.search_case_sensitive;
        let entity = cx.entity().clone();
        let background_executor = cx.background_executor().clone();

        // Cancel any in-flight search
        self.search_task = Some(cx.spawn(async move |_, cx| {
            // Wait for user to stop typing
            Timer::after(Duration::from_millis(200)).await;

            // Snapshot content and cursor on the main thread, then search in background
            let search_input = cx.update(|cx| {
                let state = entity.read(cx);
                let content = state.rope.to_string();
                let cursor_byte = state.pos_to_byte_offset(state.cursor);
                (content, cursor_byte)
            });

            let Ok((content, cursor_byte)) = search_input else {
                return;
            };

            let matches = background_executor
                .spawn(async move {
                    let mut results = Vec::new();
                    if use_regex {
                        let pattern = if case_sensitive {
                            query_owned.to_string()
                        } else {
                            format!("(?i){}", query_owned)
                        };
                        if let Ok(re) = Regex::new(&pattern) {
                            for m in re.find_iter(&content) {
                                results.push((m.start(), m.end()));
                            }
                        }
                    } else {
                        let (haystack, needle): (String, String) = if case_sensitive {
                            (content.to_string(), query_owned.to_string())
                        } else {
                            (content.to_lowercase(), query_owned.to_lowercase())
                        };
                        let needle_len = needle.len();
                        let mut start = 0;
                        while let Some(pos) = haystack[start..].find(&needle) {
                            let match_start = start + pos;
                            let match_end = match_start + needle_len;
                            results.push((match_start, match_end));
                            start = match_start + 1;
                        }
                    }
                    results
                })
                .await;

            let _ = cx.update(|cx| {
                entity.update(cx, |state, cx| {
                    state.search_matches = matches;
                    if !state.search_matches.is_empty() {
                        let idx = state
                            .search_matches
                            .iter()
                            .position(|(s, _)| *s >= cursor_byte)
                            .unwrap_or(0);
                        state.current_match_idx = Some(idx);
                        state.scroll_to_match(idx);
                    } else {
                        state.current_match_idx = None;
                    }
                    cx.notify();
                });
            });
        }));
    }

    pub fn find_next(&mut self, cx: &mut Context<Self>) {
        if self.search_matches.is_empty() {
            return;
        }
        let next = match self.current_match_idx {
            Some(idx) => (idx + 1) % self.search_matches.len(),
            None => 0,
        };
        self.current_match_idx = Some(next);
        let (start, _) = self.search_matches[next];
        self.cursor = self.byte_offset_to_pos(start);
        self.selection = None;
        self.scroll_to_match(next);
        cx.notify();
    }

    pub fn find_previous(&mut self, cx: &mut Context<Self>) {
        if self.search_matches.is_empty() {
            return;
        }
        let prev = match self.current_match_idx {
            Some(0) | None => self.search_matches.len() - 1,
            Some(idx) => idx - 1,
        };
        self.current_match_idx = Some(prev);
        let (start, _) = self.search_matches[prev];
        self.cursor = self.byte_offset_to_pos(start);
        self.selection = None;
        self.scroll_to_match(prev);
        cx.notify();
    }

    pub fn replace_current(&mut self, replacement: &str, cx: &mut Context<Self>) {
        if self.read_only || self.disabled {
            return;
        }
        let idx = match self.current_match_idx {
            Some(i) if i < self.search_matches.len() => i,
            _ => return,
        };
        let (start, end) = self.search_matches[idx];
        let old_end_position = self.byte_to_ts_point(end.min(self.rope.len_bytes()));
        let deleted: String = self.rope.byte_slice(start..end).into();
        self.undo_stack.push(EditOp::Delete {
            byte_offset: start,
            text: deleted,
        });
        self.rope_remove(start, end);
        self.undo_stack.push(EditOp::Insert {
            byte_offset: start,
            text: replacement.to_string(),
        });
        self.rope_insert(start, replacement);
        self.redo_stack.clear();
        self.mark_modified();
        let new_end = start + replacement.len();
        self.update_syntax_tree_incremental(start, end, new_end, old_end_position, cx);
        self.invalidate_after_edit();
        let query = self.search_query.clone();
        self.find_all(&query, cx);
    }

    pub fn replace_all(&mut self, replacement: &str, cx: &mut Context<Self>) {
        if self.read_only || self.disabled || self.search_matches.is_empty() {
            return;
        }
        let matches: Vec<_> = self.search_matches.iter().rev().copied().collect();
        for (start, end) in matches {
            let deleted: String = self.rope.byte_slice(start..end).into();
            self.undo_stack.push(EditOp::Delete {
                byte_offset: start,
                text: deleted,
            });
            self.rope_remove(start, end);
            self.undo_stack.push(EditOp::Insert {
                byte_offset: start,
                text: replacement.to_string(),
            });
            self.rope_insert(start, replacement);
        }
        self.redo_stack.clear();
        self.mark_modified();
        self.update_syntax_tree();
        self.invalidate_after_edit();
        let query = self.search_query.clone();
        self.find_all(&query, cx);
    }

    /// Full invalidation — clears all caches. Use for structural changes
    /// (file load, language change, fold/unfold).
    fn invalidate_all_caches(&mut self) {
        self.line_layouts.clear();
        self.line_content_hashes.clear();
        self.accessibility_geometry = None;
        self.line_geometry_compatible.clear();
        self.line_text_runs.clear();
        self.line_geometry_candidates.clear();
        self.line_native_geometry.clear();
        self.highlight_cache_version = u64::MAX;
    }

    /// Invalidation for text edits. Clears all caches since line indices
    /// shift on insert/delete, making index-keyed caches stale.
    fn invalidate_after_edit(&mut self) {
        if self
            .fold_line_index
            .as_ref()
            .is_some_and(|index| index.total_lines != self.total_lines())
        {
            self.rebuild_fold_line_index();
        }
        self.line_layouts.clear();
        self.line_content_hashes.clear();
        self.accessibility_geometry = None;
        self.line_geometry_compatible.clear();
        self.line_text_runs.clear();
        self.line_geometry_candidates.clear();
        self.line_native_geometry.clear();
        self.highlight_cache_version = u64::MAX;
    }

    pub fn invalidate_line_layouts(&mut self, cx: &mut Context<Self>) {
        self.invalidate_all_caches();
        cx.notify();
    }

    pub fn clear_search(&mut self, cx: &mut Context<Self>) {
        self.search_query.clear();
        self.search_matches.clear();
        self.current_match_idx = None;
        cx.notify();
    }

    pub fn set_search_case_sensitive(&mut self, val: bool, cx: &mut Context<Self>) {
        self.search_case_sensitive = val;
        if !self.search_query.is_empty() {
            let query = self.search_query.clone();
            self.find_all(&query, cx);
        } else {
            cx.notify();
        }
    }

    pub fn set_search_regex(&mut self, val: bool, cx: &mut Context<Self>) {
        self.search_use_regex = val;
        if !self.search_query.is_empty() {
            let query = self.search_query.clone();
            self.find_all(&query, cx);
        } else {
            cx.notify();
        }
    }

    pub fn goto_line(&mut self, line: usize, cx: &mut Context<Self>) {
        let target = line
            .saturating_sub(1)
            .min(self.total_lines().saturating_sub(1));
        self.cursor = Position::new(target, 0);
        self.selection = None;
        self.ensure_cursor_visible(cx);
    }

    fn scroll_to_match(&mut self, idx: usize) {
        if idx >= self.search_matches.len() {
            return;
        }
        let (start, _) = self.search_matches[idx];
        let pos = self.byte_offset_to_pos(start);
        let line_height = self.line_height;
        let padding_top = px(12.0);
        let viewport_bounds = self.scroll_handle.bounds();
        let viewport_height = viewport_bounds.size.height;
        let offset = self.scroll_handle.offset();
        let target_y = padding_top + line_height * (pos.line as f32);
        let current_top = -offset.y;
        let current_bottom = current_top + viewport_height;

        let mut new_offset_y = offset.y;
        if target_y < current_top || target_y + line_height > current_bottom {
            new_offset_y = -(target_y - viewport_height / 2.0 + line_height / 2.0);
            let max_offset = self.scroll_handle.max_offset().height;
            new_offset_y = new_offset_y.max(-max_offset).min(px(0.0));
        }

        if (new_offset_y - offset.y).abs() > px(0.0) {
            self.scroll_handle.set_offset(point(offset.x, new_offset_y));
        }
    }

    fn ensure_cursor_visible(&mut self, cx: &mut Context<Self>) {
        // A newly mounted line obtains native caret metrics during paint.
        // Finish horizontal visibility once, preserving later manual scrolling.
        self.pending_cursor_scroll = true;
        if self.cursor.line < self.total_lines()
            && self
                .display_line_index()
                .row_for_line(self.cursor.line)
                .is_none()
        {
            self.folded.retain(|fold| {
                !(self.cursor.line > fold.start_line && self.cursor.line <= fold.end_line)
            });
            self.rebuild_fold_line_index();
            self.invalidate_all_caches();
        }
        let line_height = self.line_height;
        let padding_top = px(12.0);
        let viewport_bounds = self.scroll_handle.bounds();
        let viewport_height = viewport_bounds.size.height;
        let viewport_width = viewport_bounds.size.width;
        let gutter_width = if self.show_line_numbers {
            px(80.0)
        } else {
            px(12.0)
        };
        let content_width = viewport_width - gutter_width;
        let offset = self.scroll_handle.offset();
        let mut new_offset_y = offset.y;
        let display_row = self
            .buffer_line_to_display_row(self.cursor.line)
            .unwrap_or(0);
        let cursor_y = padding_top + line_height * (display_row as f32);
        let current_top = -offset.y;
        let current_bottom = current_top + viewport_height;

        if cursor_y < current_top {
            new_offset_y = -cursor_y;
        } else if cursor_y + line_height > current_bottom {
            new_offset_y = -(cursor_y + line_height - viewport_height);
        }

        let max_offset = self.scroll_handle.max_offset().height;
        new_offset_y = new_offset_y.max(-max_offset).min(px(0.0));

        if (new_offset_y - offset.y).abs() > px(0.0) {
            self.scroll_handle.set_offset(point(offset.x, new_offset_y));
        }

        self.ensure_cursor_horizontal_visible(content_width);

        cx.notify();
    }

    fn ensure_cursor_horizontal_visible(&mut self, content_width: Pixels) -> bool {
        let before = self.scroll_offset_x;
        if self.line_layouts.contains_key(&self.cursor.line) {
            let cursor_x = self.line_caret_x(self.cursor.line, self.cursor.col);
            let visible_left = self.scroll_offset_x;
            let visible_right = visible_left + content_width - px(20.0);
            if cursor_x < visible_left {
                self.scroll_offset_x = (cursor_x - px(20.0)).max(px(0.0));
            } else if cursor_x > visible_right {
                self.scroll_offset_x = cursor_x - content_width + px(40.0);
            }
        }
        before != self.scroll_offset_x
    }

    fn finish_cursor_scroll(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.pending_cursor_scroll) {
            let gutter = if self.show_line_numbers {
                px(80.0)
            } else {
                px(12.0)
            };
            if self
                .ensure_cursor_horizontal_visible(self.scroll_handle.bounds().size.width - gutter)
            {
                cx.notify();
            }
        }
    }

    pub fn scroll_horizontal(&mut self, delta: Pixels, cx: &mut Context<Self>) {
        let viewport_bounds = self.scroll_handle.bounds();
        let gutter_width = if self.show_line_numbers {
            px(80.0)
        } else {
            px(12.0)
        };
        let content_width = viewport_bounds.size.width - gutter_width;
        let max_scroll = (self.max_line_width - content_width + px(40.0)).max(px(0.0));

        self.scroll_offset_x = (self.scroll_offset_x + delta).max(px(0.0)).min(max_scroll);
        cx.notify();
    }

    pub fn scroll_offset_x(&self) -> Pixels {
        self.scroll_offset_x
    }

    pub fn max_line_width(&self) -> Pixels {
        self.max_line_width
    }

    fn position_for_mouse(
        &self,
        mouse_pos: Point<Pixels>,
        bounds: Bounds<Pixels>,
        gutter_width: Pixels,
        line_height: Pixels,
    ) -> Position {
        let padding_top = px(12.0);
        let relative_y = mouse_pos.y - bounds.top() - padding_top;
        let display_row_f = (relative_y / line_height).floor();
        let display_lines = self.display_line_index();
        let display_count = display_lines.len();
        let display_row = if display_row_f < 0.0 {
            0
        } else {
            min(display_row_f as usize, display_count.saturating_sub(1))
        };
        let line = display_lines.line_for_row(display_row).unwrap_or(0);

        let relative_x = mouse_pos.x - bounds.left() - gutter_width + self.scroll_offset_x;
        let col = if let Some(layout) = self.line_layouts.get(&line) {
            let idx = self.native_geometry_for_line(line).map_or_else(
                || layout.closest_index_for_x(relative_x),
                |geometry| geometry.closest_byte_for_x(relative_x, layout.len()),
            );
            idx.min(self.line_len(line))
        } else {
            let approx_char_width = px(8.4);
            if relative_x > px(0.0) {
                let col = (relative_x / approx_char_width).round() as usize;
                col.min(self.line_len(line))
            } else {
                0
            }
        };

        Position::new(line, col)
    }

    fn start_autoscroll(&mut self, cx: &mut Context<Self>) {
        let entity = cx.entity().clone();
        let line_height = self.line_height;
        self.autoscroll_task = Some(cx.spawn(async move |_, cx| {
            loop {
                Timer::after(Duration::from_millis(50)).await;
                let should_continue = cx
                    .update(|cx| {
                        entity.update(cx, |state, cx| {
                            if !state.is_selecting {
                                return false;
                            }
                            let Some(mouse_pos) = state.last_mouse_pos else {
                                return true;
                            };
                            let Some(bounds) = state.last_bounds else {
                                return true;
                            };

                            let viewport_bounds = state.scroll_handle.bounds();
                            if viewport_bounds.size.height == px(0.0) {
                                return true;
                            }
                            let viewport_top = viewport_bounds.top();
                            let viewport_bottom = viewport_bounds.bottom();
                            let mouse_y = mouse_pos.y;
                            let edge_zone = line_height * 1.5;
                            let mut scrolled = false;

                            if mouse_y < viewport_top + edge_zone {
                                let speed = ((viewport_top + edge_zone - mouse_y) / edge_zone)
                                    .clamp(0.5, 5.0);
                                let offset = state.scroll_handle.offset();
                                let new_y = (offset.y + line_height * speed).min(px(0.0));
                                state.scroll_handle.set_offset(point(offset.x, new_y));
                                scrolled = true;
                            } else if mouse_y > viewport_bottom - edge_zone {
                                let speed = ((mouse_y - (viewport_bottom - edge_zone)) / edge_zone)
                                    .clamp(0.5, 5.0);
                                let offset = state.scroll_handle.offset();
                                let max_offset = state.scroll_handle.max_offset().height;
                                let new_y = (offset.y - line_height * speed).max(-max_offset);
                                state.scroll_handle.set_offset(point(offset.x, new_y));
                                scrolled = true;
                            }

                            if scrolled {
                                let gutter_width = state.last_mouse_gutter_width;
                                let pos = state.position_for_mouse(
                                    mouse_pos,
                                    bounds,
                                    gutter_width,
                                    line_height,
                                );
                                if let Some(ref mut sel) = state.selection {
                                    sel.cursor = pos;
                                } else {
                                    state.selection = Some(Selection::new(state.cursor, pos));
                                }
                                state.cursor = pos;
                                cx.notify();
                            }
                            true
                        })
                    })
                    .unwrap_or(false);
                if !should_continue {
                    break;
                }
            }
        }));
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        bounds: Bounds<Pixels>,
        gutter_width: Pixels,
        line_height: Pixels,
        _window: &Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        let click_x = event.position.x - bounds.left();
        let padding_top = px(12.0);
        let display_row = ((event.position.y - bounds.top() - padding_top) / line_height)
            .floor()
            .max(0.0) as usize;
        let click_line = self
            .display_line_index()
            .line_for_row(display_row)
            .unwrap_or(0);

        if click_x >= gutter_width - px(16.0)
            && click_x <= gutter_width
            && self.fold_ranges.iter().any(|f| f.start_line == click_line)
        {
            self.toggle_fold_at_line(click_line, cx);
            return;
        }

        let pos = self.position_for_mouse(event.position, bounds, gutter_width, line_height);

        let now = web_time::Instant::now();
        let is_double_click = if let Some(last_time) = self.last_click_time {
            now.duration_since(last_time).as_millis() < 500
        } else {
            false
        };
        self.last_click_time = Some(now);

        if is_double_click {
            self.selection = Some(Selection::new(
                Position::new(pos.line, 0),
                Position::new(pos.line, self.line_len(pos.line)),
            ));
            self.cursor = Position::new(pos.line, self.line_len(pos.line));
        } else if event.modifiers.shift {
            if let Some(ref mut sel) = self.selection {
                sel.cursor = pos;
                self.cursor = pos;
            } else {
                self.selection = Some(Selection::new(self.cursor, pos));
                self.cursor = pos;
            }
        } else {
            self.cursor = pos;
            self.selection = None;
            self.is_selecting = true;
            self.last_mouse_pos = Some(event.position);
            self.last_mouse_gutter_width = gutter_width;
            self.start_autoscroll(cx);
        }

        cx.notify();
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        bounds: Bounds<Pixels>,
        gutter_width: Pixels,
        line_height: Pixels,
        _window: &Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        if self.dragging_h_scrollbar {
            if event.pressed_button != Some(MouseButton::Left) {
                self.dragging_h_scrollbar = false;
                cx.notify();
                return;
            }
            let max_w = self.max_line_width;
            let vp = self.scroll_handle.bounds();
            let gw = if self.show_line_numbers {
                px(80.0)
            } else {
                px(12.0)
            };
            let cw = vp.size.width - gw;
            let scroll_range = max_w - cw;

            if scroll_range > px(0.0) {
                let track_width = vp.size.width;
                let click_ratio = (event.position.x - vp.left()) / track_width;
                let new_scroll = scroll_range * click_ratio;
                self.scroll_offset_x = new_scroll.max(px(0.0)).min(scroll_range);
                cx.notify();
            }
            return;
        }

        if !self.is_selecting || event.pressed_button != Some(MouseButton::Left) {
            if self.is_selecting && event.pressed_button != Some(MouseButton::Left) {
                self.is_selecting = false;
                self.last_mouse_pos = None;
                cx.notify();
            }
            return;
        }

        self.last_mouse_pos = Some(event.position);
        self.last_mouse_gutter_width = gutter_width;

        let pos = self.position_for_mouse(event.position, bounds, gutter_width, line_height);
        if let Some(ref mut sel) = self.selection {
            sel.cursor = pos;
        } else {
            self.selection = Some(Selection::new(self.cursor, pos));
        }
        self.cursor = pos;
        self.ensure_cursor_visible(cx);
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.is_selecting = false;
        self.dragging_h_scrollbar = false;
        self.autoscroll_task = None;
        self.last_mouse_pos = None;
        cx.notify();
    }
}

impl Focusable for EditorState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for EditorState {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        let start_pos = self.byte_offset_to_pos(range.start);
        let end_pos = self.byte_offset_to_pos(range.end);
        Some(self.get_selection_text(&Selection::new(start_pos, end_pos)))
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        if let Some(selection) = &self.selection {
            let start_offset = self.pos_to_byte_offset(selection.anchor);
            let end_offset = self.pos_to_byte_offset(selection.cursor);
            let range =
                self.range_to_utf16(&(start_offset.min(end_offset)..start_offset.max(end_offset)));
            Some(UTF16Selection {
                range,
                reversed: selection.anchor > selection.cursor,
            })
        } else {
            let cursor_offset = self.pos_to_byte_offset(self.cursor);
            let range = self.range_to_utf16(&(cursor_offset..cursor_offset));
            Some(UTF16Selection {
                range,
                reversed: false,
            })
        }
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.marked_range.take().is_some() {
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only || self.disabled {
            return;
        }
        let composing = self.marked_range.is_some();
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .unwrap_or_else(|| self.input_replacement_range());
        // Automatic bracket pairing applies only to ordinary insertion at the
        // caret, never a replacement or an IME commit.
        if !composing
            && range.is_empty()
            && range.start == self.pos_to_byte_offset(self.cursor)
            && self.selection.is_none()
            && new_text.len() == 1
        {
            let ch = new_text.chars().next().unwrap();
            if let Some(closer) = self.closing_char_for(ch) {
                self.replace_input_range(range, &format!("{ch}{closer}"), false, cx);
                self.cursor.col = self.cursor.col.saturating_sub(1);
                cx.notify();
                return;
            }
            if self.should_skip_closing_char(ch) {
                self.cursor.col += 1;
                cx.notify();
                return;
            }
        }
        self.replace_input_range(range, new_text, composing, cx);
        self.marked_range = None;
        self.ensure_cursor_visible(cx);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only || self.disabled {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .unwrap_or_else(|| self.input_replacement_range());
        let start = range.start;
        self.replace_input_range(range, new_text, true, cx);
        self.marked_range = (!new_text.is_empty()).then_some(start..start + new_text.len());
        if let Some(selection) = new_selected_range_utf16 {
            // Platform selection offsets are relative to the newly marked text.
            // Convert that text, rather than using offsets into the document.
            let local_offset = |requested| {
                let mut utf16 = 0;
                let mut bytes = 0;
                for ch in new_text.chars() {
                    if utf16 >= requested {
                        break;
                    }
                    utf16 += ch.len_utf16();
                    bytes += ch.len_utf8();
                }
                bytes
            };
            let anchor = self.byte_offset_to_pos(start + local_offset(selection.start));
            let cursor = self.byte_offset_to_pos(start + local_offset(selection.end));
            self.selection = Some(Selection::new(anchor, cursor));
            self.cursor = cursor;
        }
        self.ensure_cursor_visible(cx);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.bounds_for_byte_range(self.range_from_utf16(&range_utf16))
    }

    fn bounds_for_range_with_actual_range(
        &mut self,
        range_utf16: Range<usize>,
        _bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<(Bounds<Pixels>, Range<usize>)> {
        if range_utf16.start > range_utf16.end {
            return None;
        }
        // Candidate geometry starts at the containing scalar/cluster; editing
        // retains its separate forward surrogate adjustment above.
        let start = self.rope.char_to_byte(
            self.rope
                .utf16_cu_to_char(range_utf16.start.min(self.rope.len_utf16_cu())),
        );
        let end = if range_utf16.is_empty() {
            start
        } else {
            self.offset_from_utf16(range_utf16.end)
        };
        self.bounds_for_byte_range_with_actual_range(start..end)
            .map(|(bounds, actual)| (bounds, self.range_to_utf16(&actual)))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        if let Some(bounds) = self.last_bounds {
            let gutter_width = if self.show_line_numbers {
                px(80.0)
            } else {
                px(12.0)
            };
            let line_height = self.line_height;
            let pos = self.position_for_mouse(point, bounds, gutter_width, line_height);
            let offset = self.pos_to_byte_offset(pos);
            Some(self.offset_to_utf16(offset))
        } else {
            None
        }
    }
}

impl Render for EditorState {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        EditorElement { state: cx.entity() }
    }
}

struct EditorElement {
    state: Entity<EditorState>,
}

struct PrepaintState {
    gutter_width: Pixels,
    line_height: Pixels,
}

impl IntoElement for EditorElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&kael::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let line_height = self.state.read(cx).line_height;
        let padding_top = px(12.0);
        let padding_bottom = px(12.0);
        let num_lines = self.state.read(cx).display_line_count();
        let content_height = padding_top + padding_bottom + (line_height * num_lines as f32);
        let viewport_height = self.state.read(cx).scroll_handle.bounds().size.height;
        let overscroll = if viewport_height > line_height * 5.0 {
            viewport_height / 2.0
        } else {
            px(100.0)
        };
        let final_height = content_height + overscroll;

        let mut layout_style = kael::Style::default();
        layout_style.size.width = relative(1.).into();
        layout_style.size.height = final_height.into();

        (window.request_layout(layout_style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&kael::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let state = self.state.read(cx);
        let show_line_numbers = state.show_line_numbers;
        let line_height = state.line_height;
        PrepaintState {
            gutter_width: if show_line_numbers {
                px(80.0)
            } else {
                px(12.0)
            },
            line_height,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&kael::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.state.read(cx).focus_handle.clone();
        let theme = use_theme();
        let padding_top = px(12.0);
        let line_height = prepaint.line_height;
        let gutter_width = prepaint.gutter_width;
        let font_size = self.state.read(cx).font_size;

        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.state.clone()),
            cx,
        );

        self.state.update(cx, |state, _| {
            state.last_bounds = Some(bounds);
        });

        let scroll_offset = self.state.read(cx).scroll_handle.offset();
        let viewport_height = self.state.read(cx).scroll_handle.bounds().size.height;

        let display_lines = self.state.read(cx).display_line_index();
        let display_count = display_lines.len();
        let buf_to_disp = |line: usize| display_lines.row_for_line(line);

        let first_visible_display_row = ((-scroll_offset.y - padding_top) / line_height)
            .floor()
            .max(0.0) as usize;
        let visible_rows = ((viewport_height / line_height).ceil() as usize + 2).max(1);
        let last_visible_display_row = min(first_visible_display_row + visible_rows, display_count);

        let visible_buffer_lines =
            display_lines.visible_range(first_visible_display_row..last_visible_display_row);
        let visible_buffer_lines = visible_buffer_lines.as_slice();

        let (cursor, selection, show_line_numbers, scroll_offset_x) = {
            let state = self.state.read(cx);
            (
                state.cursor,
                state.selection,
                state.show_line_numbers,
                state.scroll_offset_x,
            )
        };

        let (
            gutter_bg_color,
            line_num_color,
            line_num_active_color,
            current_line_color,
            bracket_match_color,
            word_highlight_color,
            indent_guide_color,
            indent_guide_active_color,
            fold_marker_color,
            tab_size,
        ) = {
            let s = self.state.read(cx);
            (
                s.gutter_bg_override.unwrap_or(theme.tokens.background),
                s.line_number_color_override
                    .unwrap_or(theme.tokens.muted_foreground),
                s.line_number_active_color_override
                    .unwrap_or(theme.tokens.foreground),
                s.current_line_color_override
                    .unwrap_or(hsla(0.0, 0.0, 1.0, 0.06)),
                s.bracket_match_color_override
                    .unwrap_or(hsla(0.58, 0.70, 0.65, 0.60)),
                s.word_highlight_color_override
                    .unwrap_or(hsla(0.0, 0.0, 1.0, 0.08)),
                s.indent_guide_color_override
                    .unwrap_or(hsla(0.0, 0.0, 1.0, 0.06)),
                s.indent_guide_active_color_override
                    .unwrap_or(hsla(0.0, 0.0, 1.0, 0.15)),
                s.fold_marker_color_override
                    .unwrap_or(theme.tokens.muted_foreground),
                s.tab_size,
            )
        };

        let is_focused = focus_handle.is_focused(window);
        let is_single_cursor =
            selection.is_none() || selection.as_ref().map(|s| s.is_empty()).unwrap_or(true);

        if is_focused
            && is_single_cursor
            && let Some(display_row) = buf_to_disp(cursor.line)
            && display_row >= first_visible_display_row
            && display_row < last_visible_display_row
        {
            let hl_y = bounds.top() + padding_top + line_height * display_row as f32;
            window.paint_quad(fill(
                Bounds::new(
                    point(bounds.left(), hl_y),
                    size(bounds.size.width, line_height),
                ),
                current_line_color,
            ));
        }

        // Cache highlight spans — only recompute when content changes or visible range shifts
        let content_version = self.state.read(cx).content_version;
        let first_buf = visible_buffer_lines.first().copied().unwrap_or(0);
        let last_buf = visible_buffer_lines.last().copied().unwrap_or(0) + 1;
        {
            let state = self.state.read(cx);
            let needs_rehighlight = state.highlight_cache_version != content_version
                || first_buf < state.highlight_cache_first_line
                || last_buf > state.highlight_cache_last_line;
            if needs_rehighlight {
                let spans = self.collect_highlight_spans_for_lines(visible_buffer_lines, cx);
                self.state.update(cx, |state, _| {
                    state.cached_highlight_spans = spans;
                    state.highlight_cache_version = content_version;
                    state.highlight_cache_first_line = first_buf;
                    state.highlight_cache_last_line = last_buf;
                });
            }
        }

        let text_style = window.text_style();
        let mut shaped_layouts: Vec<(usize, Option<(ShapedLine, Vec<TextRun>)>, u64)> =
            Vec::with_capacity(visible_buffer_lines.len());
        let mut max_line_width = px(0.0);

        let char_width = {
            let space_run = TextRun {
                len: 1,
                font: text_style.font(),
                color: theme.tokens.foreground,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped_space =
                window
                    .text_system()
                    .shape_line(" ".into(), font_size, &[space_run], None);
            shaped_space.x_for_index(1)
        };

        let cursor_indent = {
            let cursor_line_text = self.state.read(cx).line_text(cursor.line);
            let cursor_leading = cursor_line_text.len() - cursor_line_text.trim_start().len();
            cursor_leading.checked_div(tab_size).unwrap_or(0)
        };

        for display_row in first_visible_display_row..last_visible_display_row {
            let line_idx = display_lines
                .line_for_row(display_row)
                .expect("viewport row");
            let y = bounds.top() + padding_top + line_height * display_row as f32;

            let line_text = self.state.read(cx).line_text(line_idx);
            let leading_spaces = line_text.len() - line_text.trim_start().len();
            let indent_levels = leading_spaces.checked_div(tab_size).unwrap_or(0);

            for level in 0..indent_levels {
                let guide_x = bounds.left() + gutter_width + char_width * (level * tab_size) as f32
                    - scroll_offset_x;
                let color = if level == cursor_indent.saturating_sub(1) && is_focused {
                    indent_guide_active_color
                } else {
                    indent_guide_color
                };
                window.paint_quad(fill(
                    Bounds::new(point(guide_x, y), size(px(1.0), line_height)),
                    color,
                ));
            }

            // Content-hash-based cache: only re-shape lines whose content changed
            let line_hash = {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                line_text.hash(&mut hasher);
                hasher.finish()
            };
            let cached_layout = {
                let state = self.state.read(cx);
                if state.line_content_hashes.get(&line_idx) == Some(&line_hash) {
                    state.line_layouts.get(&line_idx).cloned()
                } else {
                    None
                }
            };
            if let Some(cached) = cached_layout {
                let line_width = cached.x_for_index(cached.len());
                if line_width > max_line_width {
                    max_line_width = line_width;
                }
                let _ = cached.paint(
                    point(bounds.left() + gutter_width - scroll_offset_x, y),
                    line_height,
                    window,
                    cx,
                );
                continue;
            }

            if line_text.is_empty() {
                shaped_layouts.push((line_idx, None, line_hash));
                continue;
            }

            let highlight_spans = &self.state.read(cx).cached_highlight_spans;
            let text_runs =
                self.build_text_runs(&line_text, line_idx, highlight_spans, &text_style, &theme);

            let line_len = line_text.len();
            let shaped =
                window
                    .text_system()
                    .shape_line(line_text.into(), font_size, &text_runs, None);

            let line_width = shaped.x_for_index(line_len);
            if line_width > max_line_width {
                max_line_width = line_width;
            }

            let _ = shaped.paint(
                point(bounds.left() + gutter_width - scroll_offset_x, y),
                line_height,
                window,
                cx,
            );

            shaped_layouts.push((line_idx, Some((shaped, text_runs)), line_hash));
        }

        self.state.update(cx, |state, _| {
            state
                .line_layouts
                .retain(|line_idx, _| visible_buffer_lines.binary_search(line_idx).is_ok());
            state
                .line_content_hashes
                .retain(|line_idx, _| visible_buffer_lines.binary_search(line_idx).is_ok());
            state
                .line_geometry_compatible
                .retain(|line_idx, _| visible_buffer_lines.binary_search(line_idx).is_ok());
            state
                .line_text_runs
                .retain(|line, _| visible_buffer_lines.binary_search(line).is_ok());
            state
                .line_geometry_candidates
                .retain(|line, _| visible_buffer_lines.binary_search(line).is_ok());
            state
                .line_native_geometry
                .retain(|line, _| visible_buffer_lines.binary_search(line).is_ok());
            for (idx, layout, hash) in shaped_layouts {
                state.line_geometry_compatible.remove(&idx);
                state.line_native_geometry.remove(&idx);
                state.line_geometry_candidates.remove(&idx);
                state.line_text_runs.remove(&idx);
                if let Some((shaped, text_runs)) = layout {
                    state.line_text_runs.insert(idx, text_runs);
                    state.line_layouts.insert(idx, shaped);
                }
                state.line_content_hashes.insert(idx, hash);
            }
            if max_line_width > state.max_line_width {
                state.max_line_width = max_line_width;
            }
        });

        if let Some(geometry) = self.state.update(cx, |state, _| {
            state.prepare_accessibility_geometry(
                bounds,
                first_visible_display_row,
                last_visible_display_row,
                visible_buffer_lines,
                window.text_system(),
            )
        }) {
            window.set_accessibility_text_geometry(self.state.read(cx).accessibility_id, geometry);
        }
        self.state
            .update(cx, |state, cx| state.finish_accessibility_reveal(cx));
        self.state
            .update(cx, |state, cx| state.finish_cursor_scroll(cx));

        if show_line_numbers {
            window.paint_quad(PaintQuad {
                bounds: Bounds {
                    origin: bounds.origin,
                    size: Size {
                        width: gutter_width,
                        height: bounds.size.height,
                    },
                },
                corner_radii: Corners::default(),
                background: gutter_bg_color.into(),
                border_widths: Edges::default(),
                border_color: (Hsla::transparent_black()).into(),
                border_style: BorderStyle::default(),
                continuous_corners: false,
                transform: Default::default(),
                blend_mode: Default::default(),
            });

            let mut line_num_buf2 = String::with_capacity(8);
            for display_row in first_visible_display_row..last_visible_display_row {
                let line_idx = display_lines
                    .line_for_row(display_row)
                    .expect("viewport row");
                let y = bounds.top() + padding_top + line_height * display_row as f32;
                let is_current_line = line_idx == cursor.line;
                let num_color = if is_current_line && is_focused {
                    line_num_active_color
                } else {
                    line_num_color
                };
                line_num_buf2.clear();
                use std::fmt::Write;
                let _ = write!(line_num_buf2, "{:>4}", line_idx + 1);
                let num_font = if is_current_line && is_focused {
                    let mut f = text_style.font();
                    f.weight = FontWeight::BOLD;
                    f
                } else {
                    text_style.font()
                };
                let line_num_run = TextRun {
                    len: line_num_buf2.len(),
                    font: num_font,
                    color: num_color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let shaped = window.text_system().shape_line(
                    SharedString::from(line_num_buf2.clone()),
                    font_size,
                    &[line_num_run],
                    None,
                );
                let _ = shaped.paint(point(bounds.left() + px(6.0), y), line_height, window, cx);

                let fold_start = self
                    .state
                    .read(cx)
                    .fold_ranges
                    .binary_search_by_key(&line_idx, |fold| fold.start_line)
                    .is_ok();
                let is_folded = display_lines.is_fold_header(line_idx);
                if fold_start {
                    let icon_name = if is_folded {
                        "chevron-right"
                    } else {
                        "chevron-down"
                    };
                    let icon_path = SharedString::from(resolve_icon_path(icon_name));
                    let icon_size = px(16.0);
                    let icon_x = bounds.left() + gutter_width - px(18.0);
                    let icon_y = y + (line_height - icon_size) / 2.0;
                    let icon_bounds =
                        Bounds::new(point(icon_x, icon_y), size(icon_size, icon_size));
                    let _ = window.paint_svg(
                        icon_bounds,
                        icon_path,
                        TransformationMatrix::default(),
                        fold_marker_color,
                        cx,
                    );
                }
            }
        }

        if is_focused && is_single_cursor {
            let word_occurrences = self.find_word_occurrences(visible_buffer_lines, cx);
            for (occ_line, occ_start, occ_end) in &word_occurrences {
                if let Some(dr) = buf_to_disp(*occ_line)
                    && let Some(layout) = self.state.read(cx).line_layouts.get(occ_line)
                {
                    let occ_y = bounds.top() + padding_top + line_height * dr as f32;
                    let x_start = layout.x_for_index(*occ_start);
                    let x_end = layout.x_for_index(*occ_end);
                    window.paint_quad(fill(
                        Bounds::new(
                            point(
                                bounds.left() + gutter_width + x_start - scroll_offset_x,
                                occ_y,
                            ),
                            size(x_end - x_start, line_height),
                        ),
                        word_highlight_color,
                    ));
                }
            }
        }

        let sel_color = self
            .state
            .read(cx)
            .selection_color_override
            .unwrap_or(theme.tokens.primary.opacity(0.25));

        if let Some(selection) = &selection {
            let (start, end) = selection.range();
            // A Select All range can cover millions of logical rows; only the
            // mounted source rows can contribute painted selection fragments.
            for &line_idx in visible_buffer_lines {
                if line_idx < start.line || line_idx > end.line {
                    continue;
                }
                let dr = match buf_to_disp(line_idx) {
                    Some(d) => d,
                    None => continue,
                };
                if dr < first_visible_display_row || dr >= last_visible_display_row {
                    continue;
                }
                let line_y = bounds.top() + padding_top + line_height * dr as f32;
                let line_len = self.state.read(cx).line_len(line_idx);
                let start_col = if line_idx == start.line { start.col } else { 0 };
                let end_col = if line_idx == end.line {
                    end.col
                } else {
                    line_len
                };

                for rectangle in self
                    .state
                    .read(cx)
                    .line_range_rectangles(line_idx, start_col..end_col)
                {
                    window.paint_quad(fill(
                        Bounds::new(
                            point(
                                bounds.left() + gutter_width + rectangle.start - scroll_offset_x,
                                line_y,
                            ),
                            size(rectangle.end - rectangle.start, line_height),
                        ),
                        sel_color,
                    ));
                }
            }
        }

        {
            let state = self.state.read(cx);
            let (search_normal, search_active) = state
                .search_match_color_overrides
                .unwrap_or((rgba(0xFFD70040).into(), rgba(0xFF990060).into()));
            let current_match = state.current_match_idx;
            for (match_idx, &(match_start, match_end)) in state.search_matches.iter().enumerate() {
                let start_pos = state.byte_offset_to_pos(match_start);
                let end_pos = state.byte_offset_to_pos(match_end);
                let is_current = current_match == Some(match_idx);
                let color = if is_current {
                    search_active
                } else {
                    search_normal
                };

                for line_idx in start_pos.line..=end_pos.line {
                    let dr = match buf_to_disp(line_idx) {
                        Some(d) => d,
                        None => continue,
                    };
                    if dr < first_visible_display_row || dr >= last_visible_display_row {
                        continue;
                    }
                    let line_y = bounds.top() + padding_top + line_height * dr as f32;
                    let sc = if line_idx == start_pos.line {
                        start_pos.col
                    } else {
                        0
                    };
                    let ec = if line_idx == end_pos.line {
                        end_pos.col
                    } else {
                        state.line_len(line_idx)
                    };

                    let (hx, hw) = if let Some(layout) = state.line_layouts.get(&line_idx) {
                        let x_start = layout.x_for_index(sc);
                        let x_end = layout.x_for_index(ec);
                        (
                            bounds.left() + gutter_width + x_start - scroll_offset_x,
                            x_end - x_start,
                        )
                    } else {
                        continue;
                    };

                    window.paint_quad(fill(
                        Bounds::new(point(hx, line_y), size(hw, line_height)),
                        color,
                    ));
                }
            }
        }

        if is_focused && let Some((pos_a, pos_b)) = self.state.read(cx).find_matching_bracket() {
            for pos in [pos_a, pos_b] {
                if let Some(dr) = buf_to_disp(pos.line)
                    && dr >= first_visible_display_row
                    && dr < last_visible_display_row
                    && let Some(layout) = self.state.read(cx).line_layouts.get(&pos.line)
                {
                    let bx = bounds.left() + gutter_width + layout.x_for_index(pos.col)
                        - scroll_offset_x;
                    let by = bounds.top() + padding_top + line_height * dr as f32;
                    let bw = layout.x_for_index(pos.col + 1) - layout.x_for_index(pos.col);
                    let bracket_bounds = Bounds::new(point(bx, by), size(bw, line_height));
                    window.paint_quad(PaintQuad {
                        bounds: bracket_bounds,
                        corner_radii: Corners::default(),
                        background: bracket_match_color.opacity(0.3).into(),
                        border_widths: Edges::all(px(1.0)),
                        border_color: (bracket_match_color).into(),
                        border_style: BorderStyle::default(),
                        continuous_corners: false,
                        transform: Default::default(),
                        blend_mode: Default::default(),
                    });
                }
            }
        }

        {
            let diagnostics = &self.state.read(cx).diagnostics;
            if !diagnostics.is_empty() {
                for diag in diagnostics {
                    let diag_line = diag.start_line as usize;
                    let dr = match buf_to_disp(diag_line) {
                        Some(d) => d,
                        None => continue,
                    };
                    if dr < first_visible_display_row || dr >= last_visible_display_row {
                        continue;
                    }

                    let underline_color = match diag.severity {
                        DiagnosticSeverity::Error => self
                            .state
                            .read(cx)
                            .diagnostic_error_color
                            .unwrap_or(hsla(0.0, 0.85, 0.6, 1.0)),
                        DiagnosticSeverity::Warning => self
                            .state
                            .read(cx)
                            .diagnostic_warning_color
                            .unwrap_or(hsla(0.12, 0.85, 0.55, 1.0)),
                        DiagnosticSeverity::Information => self
                            .state
                            .read(cx)
                            .diagnostic_info_color
                            .unwrap_or(hsla(0.6, 0.7, 0.6, 1.0)),
                        DiagnosticSeverity::Hint => self
                            .state
                            .read(cx)
                            .diagnostic_hint_color
                            .unwrap_or(hsla(0.0, 0.0, 0.5, 0.6)),
                    };

                    let diag_y = bounds.top() + padding_top + line_height * dr as f32 + line_height
                        - px(2.0);

                    if let Some(layout) = self.state.read(cx).line_layouts.get(&diag_line) {
                        let start_col = diag.start_col as usize;
                        let end_col = if diag.end_line == diag.start_line {
                            (diag.end_col as usize).max(start_col + 1)
                        } else {
                            self.state.read(cx).line_len(diag_line)
                        };
                        let x_start = layout.x_for_index(start_col);
                        let x_end = layout.x_for_index(end_col);
                        let underline_width = (x_end - x_start).max(char_width);
                        window.paint_quad(fill(
                            Bounds::new(
                                point(
                                    bounds.left() + gutter_width + x_start - scroll_offset_x,
                                    diag_y,
                                ),
                                size(underline_width, px(2.0)),
                            ),
                            underline_color,
                        ));
                    }

                    if show_line_numbers {
                        let dot_size = px(6.0);
                        let dot_x = bounds.left() + px(2.0);
                        let dot_y = bounds.top()
                            + padding_top
                            + line_height * dr as f32
                            + (line_height - dot_size) / 2.0;
                        window.paint_quad(PaintQuad {
                            bounds: Bounds::new(point(dot_x, dot_y), size(dot_size, dot_size)),
                            corner_radii: Corners::all(dot_size / 2.0),
                            background: underline_color.into(),
                            border_widths: Edges::default(),
                            border_color: (Hsla::transparent_black()).into(),
                            border_style: BorderStyle::default(),
                            continuous_corners: false,
                            transform: Default::default(),
                            blend_mode: Default::default(),
                        });
                    }
                }
            }
        }

        if is_focused {
            let cursor_moved = {
                let state = self.state.read(cx);
                state.last_blink_cursor != cursor
            };
            if cursor_moved {
                self.state.update(cx, |state, cx| {
                    state.last_blink_cursor = cursor;
                    state.reset_cursor_blink(cx);
                    state.paint_cursor_blink(window, cx);
                });
            } else if self.state.read(cx).blink_task.is_none() {
                self.state.update(cx, |state, cx| {
                    state.paint_cursor_blink(window, cx);
                });
            } else {
                self.state
                    .update(cx, |state, cx| state.paint_cursor_blink(window, cx));
            }

            let cursor_visible = self.state.read(cx).cursor_visible;
            if cursor_visible && let Some(cursor_display_row) = buf_to_disp(cursor.line) {
                let total = self.state.read(cx).total_lines();
                let cursor_col = if cursor.line < total {
                    cursor.col.min(self.state.read(cx).line_len(cursor.line))
                } else {
                    0
                };
                let cursor_y = bounds.top() + padding_top + line_height * cursor_display_row as f32;
                let cursor_x = bounds.left()
                    + gutter_width
                    + self.state.read(cx).line_caret_x(cursor.line, cursor_col)
                    - scroll_offset_x;

                let cursor_draw_color = self
                    .state
                    .read(cx)
                    .cursor_color_override
                    .unwrap_or(theme.tokens.primary);

                window.paint_quad(fill(
                    Bounds::new(point(cursor_x, cursor_y), size(px(2.0), line_height)),
                    cursor_draw_color,
                ));
            }
        }
    }
}

struct HighlightSpan {
    line: usize,
    start_col: usize,
    end_col: usize,
    color: Hsla,
}

impl EditorElement {
    fn find_word_occurrences(
        &self,
        visible_lines: &[usize],
        cx: &App,
    ) -> Vec<(usize, usize, usize)> {
        let state = self.state.read(cx);
        let word = match state.word_under_cursor_full() {
            Some((w, _, _)) => w,
            None => return Vec::new(),
        };
        let mut results = Vec::new();
        for &line_idx in visible_lines {
            let line_text = state.line_text(line_idx);
            let mut search_from = 0;
            while let Some(pos) = line_text[search_from..].find(&word) {
                let abs_start = search_from + pos;
                let abs_end = abs_start + word.len();
                let before_ok = abs_start == 0
                    || !line_text.as_bytes()[abs_start - 1].is_ascii_alphanumeric()
                        && line_text.as_bytes()[abs_start - 1] != b'_';
                let after_ok = abs_end >= line_text.len()
                    || !line_text.as_bytes()[abs_end].is_ascii_alphanumeric()
                        && line_text.as_bytes()[abs_end] != b'_';
                if before_ok && after_ok {
                    results.push((line_idx, abs_start, abs_end));
                }
                search_from = abs_start + 1;
            }
        }
        results
    }

    fn collect_highlight_spans_for_lines(
        &self,
        visible_lines: &[usize],
        cx: &App,
    ) -> Vec<HighlightSpan> {
        if visible_lines.is_empty() {
            return Vec::new();
        }

        let state = self.state.read(cx);
        let tree = match &state.syntax_tree {
            Some(t) => t,
            None => return Vec::new(),
        };

        let query = match &state.highlight_query {
            Some(q) => q,
            None => return Vec::new(),
        };

        let rope = &state.rope;
        let total_lines = rope.len_lines();
        let mut spans = Vec::new();

        let mut chunk_start = 0usize;
        while chunk_start < visible_lines.len() {
            let mut chunk_end = chunk_start;
            while chunk_end + 1 < visible_lines.len()
                && visible_lines[chunk_end + 1] == visible_lines[chunk_end] + 1
            {
                chunk_end += 1;
            }

            let first_line = visible_lines[chunk_start];
            let last_line = visible_lines[chunk_end] + 1;

            let first_byte = rope.line_to_byte(first_line);
            let last_byte = if last_line < total_lines {
                rope.line_to_byte(last_line)
            } else {
                rope.len_bytes()
            };

            let mut cursor = QueryCursor::new();
            cursor.set_byte_range(first_byte..last_byte);

            let mut matches = cursor.matches(query, tree.root_node(), |node: tree_sitter::Node| {
                let range = node.byte_range();
                let text: String = rope
                    .byte_slice(range.start..range.end.min(rope.len_bytes()))
                    .into();
                std::iter::once(text)
            });

            while let Some(m) = matches.next() {
                for capture in m.captures {
                    let capture_name = &query.capture_names()[capture.index as usize];
                    let node = capture.node;
                    let start_byte = node.start_byte();
                    let end_byte = node.end_byte();
                    let color = if let Some(ref color_fn) = state.syntax_color_fn {
                        color_fn(capture_name)
                    } else {
                        highlight_color_for_capture(capture_name)
                    };

                    let start_line = rope.byte_to_line(start_byte);
                    let end_line =
                        rope.byte_to_line(end_byte.min(rope.len_bytes().saturating_sub(1)));

                    for line in start_line..=end_line {
                        if line < first_line || line >= last_line {
                            continue;
                        }
                        let line_start_byte = rope.line_to_byte(line);
                        let line_text = state.line_text(line);
                        let line_end_byte = line_start_byte + line_text.len();

                        let span_start = start_byte.max(line_start_byte) - line_start_byte;
                        let span_end = end_byte.min(line_end_byte) - line_start_byte;

                        if span_start < span_end {
                            spans.push(HighlightSpan {
                                line,
                                start_col: span_start,
                                end_col: span_end,
                                color,
                            });
                        }
                    }
                }
            }

            chunk_start = chunk_end + 1;
        }

        spans
    }

    fn build_text_runs(
        &self,
        line_text: &str,
        line_idx: usize,
        highlight_spans: &[HighlightSpan],
        text_style: &kael::TextStyle,
        theme: &crate::theme::Theme,
    ) -> Vec<TextRun> {
        let mut line_spans: Vec<&HighlightSpan> = highlight_spans
            .iter()
            .filter(|s| s.line == line_idx)
            .collect();
        line_spans.sort_by_key(|s| s.start_col);

        if line_spans.is_empty() {
            return vec![TextRun {
                len: line_text.len(),
                font: text_style.font(),
                color: theme.tokens.foreground,
                background_color: None,
                underline: None,
                strikethrough: None,
            }];
        }

        let text_len = line_text.len();
        let mut runs = Vec::new();
        let mut pos = 0;

        for span in &line_spans {
            let start = span.start_col.min(text_len).max(pos);
            let end = span.end_col.min(text_len);
            if end <= start {
                continue;
            }
            if start > pos {
                runs.push(TextRun {
                    len: start - pos,
                    font: text_style.font(),
                    color: theme.tokens.foreground,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                });
            }
            runs.push(TextRun {
                len: end - start,
                font: text_style.font(),
                color: span.color,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            pos = end;
        }

        if pos < text_len {
            runs.push(TextRun {
                len: text_len - pos,
                font: text_style.font(),
                color: theme.tokens.foreground,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        }

        let total_len: usize = runs.iter().map(|r| r.len).sum();
        if runs.is_empty() || total_len != text_len {
            return vec![TextRun {
                len: text_len,
                font: text_style.font(),
                color: theme.tokens.foreground,
                background_color: None,
                underline: None,
                strikethrough: None,
            }];
        }

        runs
    }
}

#[derive(IntoElement)]
pub struct Editor {
    disabled: Option<bool>,
    state: Entity<EditorState>,
    accessibility_label: SharedString,
    min_lines: Option<usize>,
    max_lines: Option<usize>,
    show_border: bool,
    style: StyleRefinement,
    cursor_color: Option<Hsla>,
    selection_color: Option<Hsla>,
    line_number_color: Option<Hsla>,
    line_number_active_color: Option<Hsla>,
    gutter_bg: Option<Hsla>,
    search_match_colors: Option<(Hsla, Hsla)>,
    current_line_color: Option<Hsla>,
    bracket_match_color: Option<Hsla>,
    word_highlight_color: Option<Hsla>,
    indent_guide_color: Option<Hsla>,
    indent_guide_active_color: Option<Hsla>,
    fold_marker_color: Option<Hsla>,
    syntax_color_fn: Option<Box<dyn Fn(&str) -> Hsla>>,
}

impl Editor {
    pub fn new(state: &Entity<EditorState>) -> Self {
        Self {
            state: state.clone(),
            disabled: None,
            accessibility_label: "Code editor".into(),
            min_lines: None,
            max_lines: None,
            show_border: true,
            style: StyleRefinement::default(),
            cursor_color: None,
            selection_color: None,
            line_number_color: None,
            line_number_active_color: None,
            gutter_bg: None,
            search_match_colors: None,
            current_line_color: None,
            bracket_match_color: None,
            word_highlight_color: None,
            indent_guide_color: None,
            indent_guide_active_color: None,
            fold_marker_color: None,
            syntax_color_fn: None,
        }
    }

    /// Set the label announced for the editor by assistive technology.
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = label.into();
        self
    }

    /// Disable user/native input while keeping complete readable text visible.
    /// Omission preserves the controller's `set_disabled` configuration.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = Some(disabled);
        self
    }

    pub fn content(self, content: impl Into<String>, cx: &mut App) -> Self {
        self.state.update(cx, |state, cx| {
            state.set_content(&content.into(), cx);
        });
        self
    }

    pub fn min_lines(mut self, lines: usize) -> Self {
        self.min_lines = Some(lines);
        self
    }

    pub fn max_lines(mut self, lines: usize) -> Self {
        self.max_lines = Some(lines);
        self
    }

    pub fn show_border(mut self, show: bool) -> Self {
        self.show_border = show;
        self
    }

    pub fn show_line_numbers(self, show: bool, cx: &mut App) -> Self {
        self.state.update(cx, |state, cx| {
            state.show_line_numbers = show;
            cx.notify();
        });
        self
    }

    pub fn cursor_color(mut self, color: Hsla) -> Self {
        self.cursor_color = Some(color);
        self
    }

    pub fn selection_color(mut self, color: Hsla) -> Self {
        self.selection_color = Some(color);
        self
    }

    pub fn line_number_color(mut self, color: Hsla) -> Self {
        self.line_number_color = Some(color);
        self
    }

    pub fn line_number_active_color(mut self, color: Hsla) -> Self {
        self.line_number_active_color = Some(color);
        self
    }

    pub fn gutter_bg(mut self, color: Hsla) -> Self {
        self.gutter_bg = Some(color);
        self
    }

    pub fn search_match_colors(mut self, normal: Hsla, active: Hsla) -> Self {
        self.search_match_colors = Some((normal, active));
        self
    }

    pub fn current_line_color(mut self, color: Hsla) -> Self {
        self.current_line_color = Some(color);
        self
    }

    pub fn bracket_match_color(mut self, color: Hsla) -> Self {
        self.bracket_match_color = Some(color);
        self
    }

    pub fn word_highlight_color(mut self, color: Hsla) -> Self {
        self.word_highlight_color = Some(color);
        self
    }

    pub fn indent_guide_colors(mut self, normal: Hsla, active: Hsla) -> Self {
        self.indent_guide_color = Some(normal);
        self.indent_guide_active_color = Some(active);
        self
    }

    pub fn fold_marker_color(mut self, color: Hsla) -> Self {
        self.fold_marker_color = Some(color);
        self
    }

    pub fn syntax_color_fn(mut self, f: impl Fn(&str) -> Hsla + 'static) -> Self {
        self.syntax_color_fn = Some(Box::new(f));
        self
    }

    pub fn get_content(&self, cx: &App) -> String {
        self.state.read(cx).content()
    }

    pub fn visual_override_count(&self) -> usize {
        [
            self.cursor_color.is_some(),
            self.selection_color.is_some(),
            self.line_number_color.is_some(),
            self.line_number_active_color.is_some(),
            self.gutter_bg.is_some(),
            self.search_match_colors.is_some(),
            self.current_line_color.is_some(),
            self.bracket_match_color.is_some(),
            self.word_highlight_color.is_some(),
            self.indent_guide_color.is_some(),
            self.indent_guide_active_color.is_some(),
            self.fold_marker_color.is_some(),
            self.syntax_color_fn.is_some(),
        ]
        .into_iter()
        .filter(|is_set| *is_set)
        .count()
    }

    /// Content-safe editor element summary for diagnostics and agent inspection.
    pub fn to_text(&self, cx: &App) -> String {
        format!(
            "editor(min_lines_set={}, max_lines_set={}, border={}, visual_override_count={}, state={})",
            self.min_lines.is_some(),
            self.max_lines.is_some(),
            self.show_border,
            self.visual_override_count(),
            self.state.read(cx).to_text()
        )
    }
}

impl Styled for Editor {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Editor {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let syn_fn = self.syntax_color_fn.take();
        self.state.update(cx, |state, cx| {
            if let Some(disabled) = self.disabled {
                state.set_disabled(disabled, cx);
            }
            state.bind_cursor_blink(window, cx);
            state.prepare_accessibility_document(cx);
            state.cursor_color_override = self.cursor_color;
            state.selection_color_override = self.selection_color;
            state.line_number_color_override = self.line_number_color;
            state.line_number_active_color_override = self.line_number_active_color;
            state.gutter_bg_override = self.gutter_bg;
            state.search_match_color_overrides = self.search_match_colors;
            state.current_line_color_override = self.current_line_color;
            state.bracket_match_color_override = self.bracket_match_color;
            state.word_highlight_color_override = self.word_highlight_color;
            state.indent_guide_color_override = self.indent_guide_color;
            state.indent_guide_active_color_override = self.indent_guide_active_color;
            state.fold_marker_color_override = self.fold_marker_color;
            state.syntax_color_fn = syn_fn;
        });
        let theme = Theme::of(cx);
        let font_family_for_editor = self
            .state
            .read(cx)
            .font_family_override
            .clone()
            .unwrap_or_else(|| theme.tokens.font_mono.clone());
        let min_height = self.min_lines.map(|lines| px(lines as f32 * 20.0));
        let max_height = self.max_lines.map(|lines| px(lines as f32 * 20.0));
        let scroll_handle = self.state.read(cx).scroll_handle.clone();

        let (accessibility_document, selection, read_only, disabled, focus_handle) = {
            let state = self.state.read(cx);
            let (anchor, focus) = state.selection_bytes();
            (
                state.prepared_accessibility_document(),
                AccessibilityTextSelection { anchor, focus },
                state.read_only,
                state.disabled,
                state.focus_handle(cx),
            )
        };
        let mut accessibility_state = AccessibilityState::NONE;
        if disabled {
            accessibility_state |= AccessibilityState::DISABLED;
            if focus_handle.is_focused(window) {
                window.blur();
            }
        }
        if read_only {
            accessibility_state |= AccessibilityState::READ_ONLY;
        }
        if focus_handle.is_focused(window) {
            accessibility_state |= AccessibilityState::FOCUSED;
        }
        let mut accessibility_actions = vec![AccessibilityAction::Focus];
        if !read_only {
            accessibility_actions.push(AccessibilityAction::SetValue);
        }
        if accessibility_document.is_some() {
            accessibility_actions.push(AccessibilityAction::SetTextSelection);
            accessibility_actions.push(AccessibilityAction::ScrollToVisible);
            accessibility_actions.push(AccessibilityAction::CopyText);
            if !read_only {
                accessibility_actions.extend([
                    AccessibilityAction::ReplaceSelectedText,
                    AccessibilityAction::CutText,
                    AccessibilityAction::PasteText,
                ]);
            }
        } else {
            accessibility_state |= AccessibilityState::BUSY;
        }
        if disabled {
            accessibility_actions.clear();
        }
        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::TextInput)
            .id(self.state.read(cx).accessibility_id)
            .label(self.accessibility_label.to_string())
            .states(accessibility_state)
            .actions(accessibility_actions);
        if let Some(document) = accessibility_document {
            accessibility = accessibility.text_document(document, selection);
        } else {
            accessibility.value = Some(AccessibilityValue::Text(
                self.state.read(cx).accessibility_value(),
            ));
        }
        // The browser DOM adapter retains its current bounded value fallback;
        // native adapters query complete text ranges from the immutable document.
        #[cfg(target_arch = "wasm32")]
        {
            accessibility.value = Some(AccessibilityValue::Text(
                self.state.read(cx).accessibility_value(),
            ));
        }

        let mut base = div()
            .id(("editor", self.state.entity_id()))
            .accessibility(accessibility)
            .on_accessibility_action(AccessibilityAction::SetTextSelection, {
                let state = self.state.downgrade();
                move |request, window, cx| {
                    let Some(AccessibilityActionPayload::TextSelection {
                        document_id,
                        anchor,
                        focus,
                    }) = request.payload.as_ref()
                    else {
                        return;
                    };
                    let _ = state.update(cx, |state, cx| {
                        if state
                            .set_accessibility_selection(*document_id, *anchor, *focus, cx)
                            .is_ok()
                        {
                            window.focus(&state.focus_handle(cx));
                        }
                    });
                }
            })
            .key_context("Editor")
            .track_focus(&focus_handle.tab_index(0).tab_stop(!disabled))
            .w_full()
            .h_full()
            .max_h_full();

        base = base
            .on_accessibility_action(AccessibilityAction::SetValue, {
                let state = self.state.downgrade();
                move |request, _, cx| {
                    if let Some(AccessibilityActionPayload::Value(value)) = request.payload.as_ref()
                    {
                        let _ = state.update(cx, |state, cx| {
                            if !state.read_only && !state.disabled {
                                state.marked_range = None;
                                state.replace_input_range(
                                    0..state.rope.len_bytes(),
                                    value,
                                    false,
                                    cx,
                                );
                                state.ensure_cursor_visible(cx);
                                cx.notify();
                            }
                        });
                    }
                }
            })
            .on_accessibility_action(AccessibilityAction::ScrollToVisible, {
                let state = self.state.downgrade();
                move |request, _, cx| {
                    if let Some(AccessibilityActionPayload::TextReveal {
                        document_id,
                        start,
                        end,
                        alignment,
                    }) = request.payload.as_ref()
                    {
                        let _ = state.update(cx, |state, cx| {
                            let _ = state.reveal_accessibility_text(
                                *document_id,
                                *start,
                                *end,
                                *alignment,
                                cx,
                            );
                        });
                    }
                }
            })
            .on_accessibility_action(AccessibilityAction::ReplaceSelectedText, {
                let state = self.state.downgrade();
                move |request, _, cx| {
                    if let Some(AccessibilityActionPayload::TextReplacement {
                        document_id,
                        start,
                        end,
                        value,
                    }) = request.payload.as_ref()
                    {
                        let _ = state.update(cx, |state, cx| {
                            let _ = state.replace_accessibility_text(
                                *document_id,
                                *start,
                                *end,
                                value,
                                cx,
                            );
                        });
                    }
                }
            });
        for action in [
            AccessibilityAction::CopyText,
            AccessibilityAction::CutText,
            AccessibilityAction::PasteText,
        ] {
            base = base.on_accessibility_action(action, {
                let state = self.state.downgrade();
                move |request, _, cx| {
                    if let Some(AccessibilityActionPayload::TextSelection {
                        document_id,
                        anchor,
                        focus,
                    }) = request.payload.as_ref()
                    {
                        let _ = state.update(cx, |state, cx| {
                            let _ = state.accessibility_clipboard(
                                action,
                                *document_id,
                                *anchor,
                                *focus,
                                cx,
                            );
                        });
                    }
                }
            });
        }

        if let Some(h) = min_height {
            base = base.min_h(h);
        }
        if let Some(h) = max_height {
            base = base.max_h(h);
        }

        let styled_base = base
            .bg(theme.tokens.background)
            .rounded(theme.tokens.radius_md);

        let final_base = if self.show_border {
            styled_base.border_1().border_color(theme.tokens.border)
        } else {
            styled_base
        };

        let user_style = self.style;

        final_base
            .map(|this| {
                let mut d = this;
                d.style().refine(&user_style);
                d
            })
            .font_family(font_family_for_editor.clone())
            .on_action(window.listener_for(&self.state, EditorState::move_up))
            .on_action(window.listener_for(&self.state, EditorState::move_down))
            .on_action(window.listener_for(&self.state, EditorState::move_left))
            .on_action(window.listener_for(&self.state, EditorState::move_right))
            .on_action(window.listener_for(&self.state, EditorState::move_word_left))
            .on_action(window.listener_for(&self.state, EditorState::move_word_right))
            .on_action(window.listener_for(&self.state, EditorState::move_to_line_start))
            .on_action(window.listener_for(&self.state, EditorState::move_to_line_end))
            .on_action(window.listener_for(&self.state, EditorState::move_to_doc_start))
            .on_action(window.listener_for(&self.state, EditorState::move_to_doc_end))
            .on_action(window.listener_for(&self.state, EditorState::page_up))
            .on_action(window.listener_for(&self.state, EditorState::page_down))
            .on_action(window.listener_for(&self.state, EditorState::select_up))
            .on_action(window.listener_for(&self.state, EditorState::select_down))
            .on_action(window.listener_for(&self.state, EditorState::select_left))
            .on_action(window.listener_for(&self.state, EditorState::select_right))
            .on_action(window.listener_for(&self.state, EditorState::select_to_line_start))
            .on_action(window.listener_for(&self.state, EditorState::select_to_line_end))
            .on_action(window.listener_for(&self.state, EditorState::select_all))
            .on_action(window.listener_for(&self.state, EditorState::backspace))
            .on_action(window.listener_for(&self.state, EditorState::delete))
            .on_action(window.listener_for(&self.state, EditorState::delete_word))
            .on_action(window.listener_for(&self.state, EditorState::enter))
            .on_action(window.listener_for(&self.state, EditorState::tab))
            .on_action(window.listener_for(&self.state, EditorState::copy))
            .on_action(window.listener_for(&self.state, EditorState::cut))
            .on_action(window.listener_for(&self.state, EditorState::paste))
            .on_action(window.listener_for(&self.state, EditorState::undo))
            .on_action(window.listener_for(&self.state, EditorState::redo))
            .on_mouse_down(MouseButton::Left, {
                let state = self.state.clone();
                move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                    if state.read(cx).disabled {
                        return;
                    }
                    let (bounds, gutter_width, line_height) = {
                        let s = state.read(cx);
                        let b = s.last_bounds.unwrap_or_default();
                        let gw = if s.show_line_numbers {
                            px(80.0)
                        } else {
                            px(12.0)
                        };
                        let lh = s.line_height;
                        (b, gw, lh)
                    };
                    state.update(cx, |s, cx| {
                        s.on_mouse_down(event, bounds, gutter_width, line_height, window, cx);
                    });
                    window.focus(&state.read(cx).focus_handle(cx));
                }
            })
            .on_mouse_move({
                let state = self.state.clone();
                move |event: &MouseMoveEvent, window: &mut Window, cx: &mut App| {
                    let (bounds, gutter_width, line_height) = {
                        let s = state.read(cx);
                        let b = s.last_bounds.unwrap_or_default();
                        let gw = if s.show_line_numbers {
                            px(80.0)
                        } else {
                            px(12.0)
                        };
                        let lh = s.line_height;
                        (b, gw, lh)
                    };
                    state.update(cx, |s, cx| {
                        s.on_mouse_move(event, bounds, gutter_width, line_height, window, cx);
                    });
                }
            })
            .on_mouse_up(
                MouseButton::Left,
                window.listener_for(&self.state, EditorState::on_mouse_up),
            )
            .on_scroll_wheel({
                let state = self.state.clone();
                move |event: &ScrollWheelEvent, _window: &mut Window, cx: &mut App| {
                    let delta_x = match event.delta {
                        ScrollDelta::Pixels(p) => p.x,
                        ScrollDelta::Lines(l) => px(l.x * 20.0),
                    };
                    if delta_x.abs() > px(0.5) {
                        state.update(cx, |s, cx| {
                            s.scroll_horizontal(-delta_x, cx);
                        });
                    }
                }
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .size_full()
                    .child(div().flex_1().overflow_hidden().child(
                        scrollable_vertical(self.state.clone()).with_scroll_handle(scroll_handle),
                    ))
                    .child(HorizontalScrollbar::new(self.state.clone(), cx)),
            )
    }
}

struct HorizontalScrollbar {
    state: Entity<EditorState>,
    needs_scrollbar: bool,
    thumb_width_pct: f32,
    thumb_left_pct: f32,
}

impl HorizontalScrollbar {
    fn new(state: Entity<EditorState>, cx: &App) -> Self {
        let s = state.read(cx);
        let max_width = s.max_line_width;
        let scroll_x = s.scroll_offset_x;
        let viewport_bounds = s.scroll_handle.bounds();
        let gutter_width = if s.show_line_numbers {
            px(80.0)
        } else {
            px(12.0)
        };
        let content_width = viewport_bounds.size.width - gutter_width;
        let needs_scrollbar = max_width > content_width && content_width > px(0.0);

        let (thumb_width_pct, thumb_left_pct) = if needs_scrollbar {
            let visible_ratio = (content_width / max_width).min(1.0);
            let twp = (visible_ratio * 100.0).max(5.0);
            let scroll_range = max_width - content_width;
            let tlp = if scroll_range > px(0.0) {
                ((scroll_x / scroll_range) * (100.0 - twp)).max(0.0)
            } else {
                0.0
            };
            (twp, tlp)
        } else {
            (0.0, 0.0)
        };

        Self {
            state,
            needs_scrollbar,
            thumb_width_pct,
            thumb_left_pct,
        }
    }
}

impl IntoElement for HorizontalScrollbar {
    type Element = AnyElement;

    fn into_element(self) -> Self::Element {
        if !self.needs_scrollbar {
            return div().h(px(0.0)).into_any_element();
        }

        let theme = use_theme();
        let editor_state = self.state.clone();
        let scrollbar_id: ElementId =
            ("editor-horizontal-scrollbar", self.state.entity_id()).into();

        div()
            .id(scrollbar_id)
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::ScrollBar)
                    .label("Horizontal editor scroll")
                    .value(AccessibilityValue::Range {
                        current: self.thumb_left_pct as f64,
                        min: 0.0,
                        max: 100.0,
                        step: None,
                    }),
            )
            .w_full()
            .h(px(12.0))
            .bg(theme.tokens.muted.opacity(0.3))
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(MouseButton::Left, {
                let state = editor_state.clone();
                move |event: &MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                    state.update(cx, |s, cx| {
                        s.dragging_h_scrollbar = true;
                        let max_w = s.max_line_width;
                        let vp = s.scroll_handle.bounds();
                        let gw = if s.show_line_numbers {
                            px(80.0)
                        } else {
                            px(12.0)
                        };
                        let cw = vp.size.width - gw;
                        let scroll_range = max_w - cw;

                        if scroll_range > px(0.0) {
                            let track_width = vp.size.width;
                            let click_ratio = (event.position.x - vp.left()) / track_width;
                            let new_scroll = scroll_range * click_ratio;
                            s.scroll_offset_x = new_scroll.max(px(0.0)).min(scroll_range);
                        }
                        cx.notify();
                    });
                }
            })
            .on_mouse_up(MouseButton::Left, {
                let state = editor_state.clone();
                move |_: &MouseUpEvent, _window, cx| {
                    state.update(cx, |s, cx| {
                        s.dragging_h_scrollbar = false;
                        cx.notify();
                    });
                }
            })
            .child(
                div()
                    .absolute()
                    .top(px(2.0))
                    .bottom(px(2.0))
                    .left(relative(self.thumb_left_pct / 100.0))
                    .w(relative(self.thumb_width_pct / 100.0))
                    .bg(theme.tokens.muted_foreground.opacity(0.6))
                    .rounded(px(3.0))
                    .hover(|s| s.bg(theme.tokens.muted_foreground.opacity(0.8))),
            )
            .into_any_element()
    }
}

#[allow(dead_code)]
struct VerticalScrollbar {
    state: Entity<EditorState>,
    needs_scrollbar: bool,
    thumb_height_pct: f32,
    thumb_top_pct: f32,
}

impl VerticalScrollbar {
    #[allow(dead_code)]
    fn new(state: Entity<EditorState>, cx: &App) -> Self {
        let s = state.read(cx);
        let line_height = s.line_height;
        let padding = px(24.0);
        let num_lines = s.display_line_count();
        let content_height = padding + (line_height * num_lines as f32);
        let viewport_height = s.scroll_handle.bounds().size.height;
        let needs_scrollbar = content_height > viewport_height && viewport_height > px(0.0);

        let (thumb_height_pct, thumb_top_pct) = if needs_scrollbar {
            let visible_ratio = (viewport_height / content_height).min(1.0);
            let thp = (visible_ratio * 100.0).max(5.0);
            let scroll_y = -s.scroll_handle.offset().y;
            let overscroll = if viewport_height > line_height * 5.0 {
                viewport_height / 2.0
            } else {
                px(100.0)
            };
            let max_scroll = content_height + overscroll - viewport_height;
            let ttp = if max_scroll > px(0.0) {
                ((scroll_y / max_scroll) * (100.0 - thp))
                    .max(0.0)
                    .min(100.0 - thp)
            } else {
                0.0
            };
            (thp, ttp)
        } else {
            (0.0, 0.0)
        };

        Self {
            state,
            needs_scrollbar,
            thumb_height_pct,
            thumb_top_pct,
        }
    }
}

impl IntoElement for VerticalScrollbar {
    type Element = AnyElement;

    fn into_element(self) -> Self::Element {
        if !self.needs_scrollbar {
            return div().w(px(0.0)).into_any_element();
        }

        let theme = use_theme();
        let editor_state = self.state.clone();
        let scrollbar_id: ElementId = ("editor-vertical-scrollbar", self.state.entity_id()).into();

        div()
            .id(scrollbar_id)
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::ScrollBar)
                    .label("Vertical editor scroll")
                    .value(AccessibilityValue::Range {
                        current: self.thumb_top_pct as f64,
                        min: 0.0,
                        max: 100.0,
                        step: None,
                    }),
            )
            .h_full()
            .w(px(12.0))
            .bg(theme.tokens.muted.opacity(0.3))
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(MouseButton::Left, {
                let state = editor_state.clone();
                move |event: &MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                    state.update(cx, |s, cx| {
                        let vp = s.scroll_handle.bounds();
                        let track_height = vp.size.height;
                        let click_ratio = (event.position.y - vp.top()) / track_height;

                        let line_height = s.line_height;
                        let padding = px(24.0);
                        let content_height =
                            padding + (line_height * s.display_line_count() as f32);
                        let overscroll = if track_height > line_height * 5.0 {
                            track_height / 2.0
                        } else {
                            px(100.0)
                        };
                        let max_scroll = content_height + overscroll - track_height;

                        if max_scroll > px(0.0) {
                            let new_scroll = max_scroll * click_ratio;
                            let offset = s.scroll_handle.offset();
                            s.scroll_handle.set_offset(point(
                                offset.x,
                                -new_scroll.max(px(0.0)).min(max_scroll),
                            ));
                        }
                        cx.notify();
                    });
                }
            })
            .child(
                div()
                    .absolute()
                    .left(px(2.0))
                    .right(px(2.0))
                    .top(relative(self.thumb_top_pct / 100.0))
                    .h(relative(self.thumb_height_pct / 100.0))
                    .bg(theme.tokens.muted_foreground.opacity(0.6))
                    .rounded(px(3.0))
                    .hover(|s| s.bg(theme.tokens.muted_foreground.opacity(0.8))),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kael::{TestAppContext, hsla};

    #[::core::prelude::v1::test]
    fn editor_position_selection_and_diagnostic_summaries_are_content_safe() {
        let selection = Selection::new(Position::new(4, 12), Position::new(2, 3));
        let diagnostic = EditorDiagnostic {
            start_line: 7,
            start_col: 5,
            end_line: 9,
            end_col: 14,
            severity: DiagnosticSeverity::Error,
            message: "Private customer token leaked here".to_string(),
        };

        assert_eq!(Language::TypeScript.to_text(), "typescript");
        assert_eq!(
            EditorSyntaxBackend::current(),
            if cfg!(target_arch = "wasm32") {
                EditorSyntaxBackend::PlainText
            } else {
                EditorSyntaxBackend::TreeSitter
            }
        );
        #[cfg(target_arch = "wasm32")]
        {
            assert!(!EditorSyntaxBackend::current().supports_syntax_trees());
            assert!(Language::Rust.tree_sitter_language().is_none());
            assert!(Language::Rust.highlight_query_source().is_none());
        }
        #[cfg(all(not(target_arch = "wasm32"), feature = "tree-sitter-rust"))]
        {
            assert!(Language::Rust.tree_sitter_language().is_some());
            assert!(Language::Rust.highlight_query_source().is_some());
        }
        assert_eq!(DiagnosticSeverity::Error.to_text(), "error");
        assert_eq!(Position::new(2, 3).to_text(), "position(line=2, col=3)");
        assert!(selection.to_text().contains("reversed=true"));
        assert!(selection.to_text().contains("line_span=3"));
        assert_eq!(
            FoldRange {
                start_line: 10,
                end_line: 15
            }
            .to_text(),
            "fold_range(start_line=10, end_line=15, line_span=6)"
        );

        let summary = diagnostic.to_text();
        assert!(summary.contains("severity=error"));
        assert!(summary.contains("message_len_bytes=34"));
        assert!(!summary.contains("Private customer"));
        assert!(!summary.contains("token"));
    }

    #[::core::prelude::v1::test]
    fn editor_state_summary_is_content_safe() {
        let cx = TestAppContext::single();
        let state = cx.update(|cx| cx.new(EditorState::new));

        cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.set_content("const secret = 'alpha-token';\nconsole.log(secret);", cx);
                state.language = Language::TypeScript;
                state.cursor = Position::new(1, 7);
                state.selection = Some(Selection::new(Position::new(0, 6), Position::new(0, 12)));
                state.is_modified = true;
                state.file_path = Some(PathBuf::from("/private/customer/secrets.ts"));
                state.undo_stack.push(EditOp::Insert {
                    byte_offset: 0,
                    text: "secret undo text".to_string(),
                });
                state.search_query = "alpha-token".to_string();
                state.search_matches.push((15, 26));
                state.current_match_idx = Some(0);
                state.search_case_sensitive = true;
                state.search_use_regex = true;
                state.read_only = true;
                state.fold_ranges.push(FoldRange {
                    start_line: 0,
                    end_line: 3,
                });
                state.folded.push(FoldRange {
                    start_line: 0,
                    end_line: 3,
                });
                state.diagnostics.push(EditorDiagnostic {
                    start_line: 1,
                    start_col: 0,
                    end_line: 1,
                    end_col: 12,
                    severity: DiagnosticSeverity::Warning,
                    message: "Confidential warning".to_string(),
                });
                state.cursor_color_override = Some(hsla(0.0, 0.5, 0.5, 1.0));
            });
        });

        cx.update(|cx| {
            let state = state.read(cx);
            assert_eq!(state.content_len_bytes(), 51);
            assert!(state.has_selection());
            assert!(!state.selection_is_empty());
            assert_eq!(state.undo_depth(), 1);
            assert_eq!(state.redo_depth(), 0);
            assert!(state.has_file_path());
            assert_eq!(state.search_query_len_bytes(), "alpha-token".len());
            assert!(state.has_search_query());
            assert!(state.has_current_match());
            assert_eq!(state.fold_range_count(), 1);
            assert_eq!(state.folded_range_count(), 1);
            assert_eq!(
                state.diagnostic_count_by_severity(DiagnosticSeverity::Warning),
                1
            );
            assert!(state.has_visual_overrides());

            let summary = state.to_text();
            assert!(summary.contains("language=typescript"));
            assert!(summary.contains("syntax_backend=tree-sitter"));
            assert!(summary.contains("cursor=position(line=1, col=7)"));
            assert!(summary.contains("has_selection=true"));
            assert!(summary.contains("read_only=true"));
            assert!(summary.contains("search_query_len_bytes=11"));
            assert!(summary.contains("diagnostics=1"));
            assert!(!summary.contains("alpha-token"));
            assert!(!summary.contains("secret"));
            assert!(!summary.contains("/private/customer"));
            assert!(!summary.contains("Confidential warning"));
        });
    }

    #[::core::prelude::v1::test]
    fn editor_element_summary_is_content_safe() {
        let cx = TestAppContext::single();
        let state = cx.update(|cx| cx.new(EditorState::new));

        cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.set_content("internal draft notes", cx);
                state.language = Language::Markdown;
            });

            let editor = Editor::new(&state)
                .min_lines(3)
                .max_lines(12)
                .show_border(false)
                .cursor_color(hsla(0.0, 0.5, 0.5, 1.0))
                .selection_color(hsla(0.5, 0.5, 0.5, 1.0))
                .syntax_color_fn(|_| hsla(0.1, 0.5, 0.5, 1.0));

            assert_eq!(editor.visual_override_count(), 3);
            let summary = editor.to_text(cx);
            assert!(summary.contains("min_lines_set=true"));
            assert!(summary.contains("max_lines_set=true"));
            assert!(summary.contains("border=false"));
            assert!(summary.contains("visual_override_count=3"));
            assert!(summary.contains("language=markdown"));
            assert!(!summary.contains("internal draft"));
        });
    }

    #[::core::prelude::v1::test]
    fn editor_accessibility_value_is_bounded_on_character_boundaries() {
        let cx = TestAppContext::single();
        let state = cx.update(|cx| cx.new(EditorState::new));
        let content = "界".repeat(MAX_ACCESSIBILITY_VALUE_CHARS + 10);

        cx.update(|cx| {
            state.update(cx, |state, cx| state.set_content(&content, cx));
            let value = state.read(cx).accessibility_value();
            assert_eq!(value.chars().count(), MAX_ACCESSIBILITY_VALUE_CHARS);
            assert!(value.chars().all(|character| character == '界'));
        });
    }
    struct DocumentHost {
        state: Entity<EditorState>,
        _observer: Subscription,
    }
    impl Render for DocumentHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            Editor::new(&self.state)
                .accessibility_label("Project document")
                .w(px(640.0))
                .h(px(480.0))
        }
    }
    fn document_window<'a>(
        cx: &'a mut TestAppContext,
        content: &str,
    ) -> (Entity<EditorState>, &'a mut VisualTestContext) {
        cx.update(|cx| {
            crate::init(cx);
            crate::theme::install_theme(cx, Theme::dark());
        });
        let state = cx.new(EditorState::new);
        state.update(cx, |state, cx| {
            state.set_content(content, cx);
            state.set_language(Language::Markdown);
        });
        let (_, window) = cx.add_window_view({
            let state = state.clone();
            move |_, cx| DocumentHost {
                _observer: cx.observe(&state, |_, _, cx| cx.notify()),
                state,
            }
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
            window.focus(&state.focus_handle(cx));
        });
        (state, window)
    }

    #[::core::prelude::v1::test]
    fn indexed_utf16_offsets_preserve_scalar_rounding_clamping_and_edits() {
        let mut cx = TestAppContext::single();
        let state = cx.new(EditorState::new);
        for text in [
            "a🙂𐐷e\u{301}日本\r\nאבג\n".repeat(32),
            "🙂".into(),
            String::new(),
        ] {
            state.update(&mut cx, |state, cx| state.set_content(&text, cx));
            cx.update(|cx| {
                let state = state.read(cx);
                let text = state.content();
                for byte in (0..=text.len()).chain([usize::MAX]) {
                    let mut boundary = byte.min(text.len());
                    while !text.is_char_boundary(boundary) {
                        boundary -= 1;
                    }
                    assert_eq!(
                        state.offset_to_utf16(byte),
                        text[..boundary].encode_utf16().count(),
                        "byte offset {byte}"
                    );
                }
                for utf16 in (0..=text.encode_utf16().count()).chain([usize::MAX]) {
                    let mut units = 0;
                    let mut bytes = 0;
                    for character in text.chars() {
                        if units >= utf16 {
                            break;
                        }
                        units += character.len_utf16();
                        bytes += character.len_utf8();
                    }
                    assert_eq!(state.offset_from_utf16(utf16), bytes, "UTF-16 {utf16}");
                }
            });
        }
    }

    #[::core::prelude::v1::test]
    fn ime_actual_range_covers_first_line_graphemes_and_zero_width_carets() {
        let mut cx = TestAppContext::single();
        let content = "e\u{301}🙂\r\n日本";
        let (state, window) = document_window(&mut cx, content);
        window.update(|window, cx| {
            state.update(cx, |state, cx| {
                let (first, actual) = state
                    .bounds_for_range_with_actual_range(
                        0..content.encode_utf16().count(),
                        Bounds::default(),
                        window,
                        cx,
                    )
                    .unwrap();
                assert_eq!(actual, 0..6, "include the first line's CRLF only");
                assert!(first.size.width > px(0.0));
                let (grapheme, actual) = state
                    .bounds_for_range_with_actual_range(1..2, Bounds::default(), window, cx)
                    .unwrap();
                assert_eq!(actual, 0..2);
                assert!(grapheme.size.width > px(0.0));
                let (_, actual) = state
                    .bounds_for_range_with_actual_range(3..4, Bounds::default(), window, cx)
                    .unwrap();
                assert_eq!(
                    actual,
                    2..4,
                    "interior UTF-16 surrogate expands to its scalar"
                );
                let (caret, actual) = state
                    .bounds_for_range_with_actual_range(2..2, Bounds::default(), window, cx)
                    .unwrap();
                assert_eq!(actual, 2..2);
                assert_eq!(caret.size.width, px(0.0));
                assert!(caret.size.height > px(0.0));
            });
        });
    }

    #[::core::prelude::v1::test]
    fn text_geometry_is_shaped_bounded_and_reused_across_caret_redraws() {
        let mut cx = TestAppContext::single();
        let content = "café 日本🙂 e\u{301}\n".repeat(100_000);
        let (state, window) = document_window(&mut cx, &content);
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            let geometry = window
                .accessibility_tree()
                .get(state.read(cx).accessibility_id)
                .unwrap()
                .text_geometry
                .clone()
                .unwrap();
            assert!(!geometry.runs().is_empty());
            assert!(
                geometry.runs().len() <= 27,
                "100k document only exports viewport geometry"
            );
            let line = state.read(cx).line_layouts.get(&0).unwrap().clone();
            let run = &geometry.runs()[0];
            assert_eq!(run.character_positions[0], 0.0);
            let expected_width = f32::from(line.x_for_index(1));
            assert_eq!(run.character_widths[0], expected_width);
            let document = state.read(cx).prepared_accessibility_document().unwrap();
            for byte in [0, 1, "café".len(), 0, 1] {
                state.update(cx, |state, cx| {
                    state.set_selection_bytes(byte, byte, cx).unwrap()
                });
                window.draw(cx).clear();
                let next = window
                    .accessibility_tree()
                    .get(state.read(cx).accessibility_id)
                    .unwrap()
                    .text_geometry
                    .as_ref()
                    .unwrap();
                assert!(
                    Arc::ptr_eq(&geometry, next),
                    "caret repaint shares actual geometry"
                );
                assert!(Arc::ptr_eq(
                    &document,
                    &state.read(cx).prepared_accessibility_document().unwrap()
                ));
            }
            let first_word = state.update(cx, |state, cx| {
                let utf16 = state.range_to_utf16(&(0.."café".len()));
                state
                    .bounds_for_range(utf16, Bounds::default(), window, cx)
                    .unwrap()
            });
            assert_eq!(first_word.size.height, state.read(cx).line_height);
            assert_eq!(first_word.size.width, line.x_for_index("café".len()));
            assert!(first_word.size.width < state.read(cx).scroll_handle.bounds().size.width);
            state.update(cx, |state, cx| {
                state
                    .set_selection_bytes(content.len(), content.len(), cx)
                    .unwrap()
            });
            window.draw(cx).clear();
            let state = state.read(cx);
            assert_eq!(state.buffer_line_to_display_row(100_000), Some(100_000));
            assert_eq!(state.selection_bytes(), (content.len(), content.len()));
            assert!(
                state
                    .bounds_for_byte_range(content.len()..content.len())
                    .is_some(),
                "EOF caret has real final-row geometry"
            );
        });
    }

    #[::core::prelude::v1::test]
    fn directional_native_geometry_drives_caret_pointer_selection_and_ime_fragments() {
        let mut cx = TestAppContext::single();
        let content = "abc אבג xyz";
        let (state, window) = document_window(&mut cx, content);
        window.update(|window, cx| {
            let mut logical = 0;
            let clusters = content
                .char_indices()
                .enumerate()
                .map(|(index, (byte, character))| {
                    let rtl = (4..7).contains(&index);
                    let (leading, trailing) = if rtl {
                        (
                            px(70.0 - (index - 4) as f32 * 10.0),
                            px(60.0 - (index - 4) as f32 * 10.0),
                        )
                    } else {
                        (px(logical as f32 * 10.0), px((logical + 1) as f32 * 10.0))
                    };
                    logical += 1;
                    ShapedTextCluster {
                        bytes: byte..byte + character.len_utf8(),
                        leading,
                        trailing,
                        right_to_left: rtl,
                    }
                })
                .collect();
            let native = Arc::new(LineTextGeometry::new(content, clusters, px(110.0)).unwrap());
            state.update(cx, |state, _| {
                // Inject a provider's native metrics into the headless test's
                // deterministic glyph backend, keeping its exact cached spans.
                state.line_native_geometry.get_mut(&0).unwrap().1 = Some(native.clone());
                state.accessibility_geometry = None;
            });
            window.draw(cx).clear();
            let state = state.read(cx);
            assert_eq!(state.line_caret_x(0, 4), px(70.0));
            assert_eq!(
                state.line_range_rectangles(0, 2..6),
                vec![px(20.0)..px(40.0), px(60.0)..px(70.0)]
            );
            let bounds = state.last_bounds.unwrap();
            assert_eq!(
                state.position_for_mouse(
                    point(bounds.left() + px(80.0) + px(69.0), bounds.top() + px(15.0)),
                    bounds,
                    px(80.0),
                    state.line_height,
                ),
                Position::new(0, 4)
            );
            let ime = state.bounds_for_byte_range(4..6).unwrap();
            assert_eq!(ime.left(), bounds.left() + px(80.0) + px(60.0));
            assert_eq!(ime.size.width, px(10.0));
            let (first, actual) = state.bounds_for_byte_range_with_actual_range(2..6).unwrap();
            assert_eq!(actual, 2..4, "only the first contiguous logical fragment");
            assert_eq!(first.left(), bounds.left() + px(80.0) + px(20.0));
            assert_eq!(first.size.width, px(20.0));
            let geometry = &state.accessibility_geometry.as_ref().unwrap().1;
            let rtl = geometry
                .runs()
                .iter()
                .find(|run| run.direction == AccessibilityTextDirection::RightToLeft)
                .unwrap();
            assert_eq!(&*rtl.character_positions, &[0.0, 10.0, 20.0]);
            assert_eq!(&*rtl.character_widths, &[10.0; 3]);
        });
    }

    #[::core::prelude::v1::test]
    fn text_reveal_unfolds_offscreen_row_without_changing_selection_composition_or_focus() {
        let mut cx = TestAppContext::single();
        let content = "row 日本🙂\n".repeat(1000);
        let (state, window) = document_window(&mut cx, &content);
        window.run_until_parked();
        window.update(|window, cx| {
            state.update(cx, |state, cx| {
                state.folded = vec![FoldRange {
                    start_line: 10,
                    end_line: 200,
                }];
                state.rebuild_fold_line_index();
                state.set_selection_bytes("row 日".len(), 0, cx).unwrap();
                state.marked_range = Some(0..3);
            });
            window.draw(cx).clear();
            let document = state.read(cx).prepared_accessibility_document().unwrap();
            let (before_selection, before_cursor, before_history) = {
                let state = state.read(cx);
                (
                    state.selection_bytes(),
                    state.cursor(),
                    (state.undo_depth(), state.redo_depth()),
                )
            };
            let byte = state.read(cx).rope.line_to_byte(150);
            state.update(cx, |state, cx| {
                state
                    .reveal_accessibility_text(
                        document.id(),
                        byte,
                        byte + 3,
                        AccessibilityTextAlignment::Top,
                        cx,
                    )
                    .unwrap()
            });
            window.draw(cx).clear();
            window.draw(cx).clear();
            let state = state.read(cx);
            assert_eq!(state.selection_bytes(), before_selection);
            assert_eq!(state.cursor(), before_cursor);
            assert_eq!(state.marked_range, Some(0..3));
            assert_eq!((state.undo_depth(), state.redo_depth()), before_history);
            assert!(state.focus_handle.is_focused(window));
            assert!(!state.is_line_folded(150));
            assert!(state.scroll_handle.offset().y < px(-1000.0));
            assert!(state.bounds_for_byte_range(byte..byte + 3).is_some());
        });
    }

    #[::core::prelude::v1::test]
    fn native_range_clipboard_edits_are_atomic_readonly_and_stale_safe() {
        let mut cx = TestAppContext::single();
        let content = "café 日本🙂\nsecond\n";
        let (state, window) = document_window(&mut cx, content);
        window.update(|window, cx| {
            let start = content.find("日本").unwrap();
            let end = start + "日本🙂".len();
            let original = state.read(cx).prepared_accessibility_document().unwrap();
            state.update(cx, |state, cx| {
                state.set_selection_bytes(1, 1, cx).unwrap();
                state
                    .accessibility_clipboard(
                        AccessibilityAction::CopyText,
                        original.id(),
                        end,
                        start,
                        cx,
                    )
                    .unwrap();
                assert_eq!(state.selection_bytes(), (1, 1));
                assert_eq!(state.undo_depth(), 0);
                state.set_read_only(true, cx);
                assert!(
                    state
                        .accessibility_clipboard(
                            AccessibilityAction::CutText,
                            original.id(),
                            start,
                            end,
                            cx
                        )
                        .is_err()
                );
                assert!(
                    state
                        .accessibility_clipboard(
                            AccessibilityAction::PasteText,
                            original.id(),
                            start,
                            end,
                            cx
                        )
                        .is_err()
                );
                assert!(
                    state
                        .replace_accessibility_text(original.id(), start, end, "bad", cx)
                        .is_err()
                );
                state
                    .accessibility_clipboard(
                        AccessibilityAction::CopyText,
                        original.id(),
                        start,
                        end,
                        cx,
                    )
                    .unwrap();
                state.set_read_only(false, cx);
                assert!(
                    state
                        .replace_accessibility_text(original.id(), start + 1, end, "bad", cx)
                        .is_err()
                );
                state
                    .accessibility_clipboard(
                        AccessibilityAction::CutText,
                        original.id(),
                        start,
                        end,
                        cx,
                    )
                    .unwrap();
                assert_eq!(state.undo_depth(), 1);
                assert_eq!(state.content(), "café \nsecond\n");
                state.undo(&Undo, window, cx);
                assert_eq!(state.content(), content);
                state.prepare_accessibility_document(cx);
                assert!(
                    state
                        .accessibility_clipboard(
                            AccessibilityAction::CopyText,
                            original.id(),
                            start,
                            end,
                            cx
                        )
                        .is_err()
                );
                let current = state.prepared_accessibility_document().unwrap();
                state
                    .accessibility_clipboard(
                        AccessibilityAction::PasteText,
                        current.id(),
                        0,
                        "café".len(),
                        cx,
                    )
                    .unwrap();
                assert_eq!(state.content(), "日本🙂 日本🙂\nsecond\n");
                assert_eq!(state.undo_depth(), 1, "paste commits one range replacement");
                state.undo(&Undo, window, cx);
                assert_eq!(state.content(), content);
            });
            assert_eq!(
                cx.read_from_clipboard().unwrap().unwrap().text().as_deref(),
                Some("日本🙂")
            );
        });
    }

    #[::core::prelude::v1::test]
    fn deferred_text_reveal_and_edit_reject_replaced_document_then_full_value_undoes_atomically() {
        let mut cx = TestAppContext::single();
        let content = format!("original\n{}", "more 日本🙂\n".repeat(100));
        let replacement = format!("modified\n{}", "more 日本🙂\n".repeat(100));
        let (state, window) = document_window(&mut cx, &content);
        window.run_until_parked();
        window.update(|window, cx| {
            let old = state.read(cx).prepared_accessibility_document().unwrap();
            let id = state.read(cx).accessibility_id;
            let byte = state.read(cx).rope.line_to_byte(99);
            window.dispatch_accessibility_action_for_test(
                AccessibilityActionRequest::with_payload(
                    id,
                    AccessibilityAction::ScrollToVisible,
                    AccessibilityActionPayload::TextReveal {
                        document_id: old.id(),
                        start: byte,
                        end: byte + 4,
                        alignment: AccessibilityTextAlignment::Top,
                    },
                ),
            );
            window.dispatch_accessibility_action_for_test(
                AccessibilityActionRequest::with_payload(
                    id,
                    AccessibilityAction::ReplaceSelectedText,
                    AccessibilityActionPayload::TextReplacement {
                        document_id: old.id(),
                        start: 0,
                        end: 3,
                        value: "BAD".into(),
                    },
                ),
            );
            state.update(cx, |state, cx| {
                state.set_content(&replacement, cx);
                state.prepare_accessibility_document(cx);
                assert_ne!(
                    state.prepared_accessibility_document().unwrap().id(),
                    old.id()
                );
            });
            window.draw(cx).clear();
        });
        window.run_until_parked();
        window.update(|window, cx| {
            assert_eq!(state.read(cx).content(), replacement);
            assert_eq!(state.read(cx).undo_depth(), 0);
            assert_eq!(state.read(cx).scroll_handle.offset().y, px(0.0));
            window.dispatch_accessibility_action_for_test(
                AccessibilityActionRequest::with_payload(
                    state.read(cx).accessibility_id,
                    AccessibilityAction::SetValue,
                    AccessibilityActionPayload::Value("native café 日本🙂\n".into()),
                ),
            );
        });
        window.run_until_parked();
        window.update(|window, cx| {
            assert_eq!(state.read(cx).content(), "native café 日本🙂\n");
            assert_eq!(state.read(cx).undo_depth(), 1);
            state.update(cx, |state, cx| state.undo(&Undo, window, cx));
            assert_eq!(state.read(cx).content(), replacement);
        });
    }

    #[::core::prelude::v1::test]
    fn disabled_document_preserves_reading_and_rejects_queued_native_keyboard_and_ime_input() {
        let mut cx = TestAppContext::single();
        let content = "native café 日本🙂\n";
        let (state, window) = document_window(&mut cx, content);
        let id = window.update(|window, cx| {
            let id = state.read(cx).accessibility_id;
            window.dispatch_accessibility_action_for_test(
                AccessibilityActionRequest::with_payload(
                    id,
                    AccessibilityAction::SetValue,
                    AccessibilityActionPayload::Value("BAD\n".into()),
                ),
            );
            state.update(cx, |state, cx| {
                state.marked_range = Some(0..3);
                state.set_disabled(true, cx);
                assert!(state.marked_range.is_none());
                state.insert_text_at_cursor("BAD", cx);
                state.replace_text_in_range(None, "BAD", window, cx);
                state.move_right(&MoveRight, window, cx);
                assert_eq!(state.content(), content);
                assert_eq!(state.selection_bytes(), (0, 0));
                assert_eq!(state.undo_depth(), 0);
            });
            window.draw(cx).clear();
            let node = window.accessibility_tree().get(id).unwrap();
            assert!(node.states.contains(AccessibilityState::DISABLED));
            assert!(node.actions.is_empty());
            assert_eq!(node.text_document.as_ref().unwrap().text(), content);
            assert!(!state.read(cx).focus_handle.is_focused(window));
            id
        });
        window.run_until_parked();
        window.update(|window, cx| {
            assert_eq!(
                state.read(cx).content(),
                content,
                "queued pre-disable SetValue is rejected"
            );
            state.update(cx, |state, cx| state.set_disabled(false, cx));
            window.draw(cx).clear();
            assert!(
                window
                    .accessibility_tree()
                    .get(id)
                    .unwrap()
                    .actions
                    .contains(&AccessibilityAction::SetValue)
            );
            window.dispatch_accessibility_action_for_test(
                AccessibilityActionRequest::with_payload(
                    id,
                    AccessibilityAction::SetValue,
                    AccessibilityActionPayload::Value("enabled 日本🙂\n".into()),
                ),
            );
        });
        window.run_until_parked();
        window.update(|_, cx| assert_eq!(state.read(cx).content(), "enabled 日本🙂\n"));
    }

    #[::core::prelude::v1::test]
    fn retained_fold_intervals_match_nested_overlap_and_clamped_coordinates() {
        for seed in 0..100 {
            let folds = (0..20)
                .map(|index| FoldRange {
                    start_line: (seed * 17 + index * 11) % 70,
                    end_line: (seed * 17 + index * 11) % 70 + index % 12,
                })
                .collect::<Vec<_>>();
            let mut reference = Vec::new();
            let mut skip_through = None;
            for line in 0..50 {
                if skip_through.is_some_and(|end| line <= end) {
                    continue;
                }
                skip_through = None;
                reference.push(line);
                if let Some(fold) = folds.iter().find(|fold| fold.start_line == line) {
                    skip_through = Some(fold.end_line);
                }
            }
            let index = FoldLineIndex::new(50, &folds);
            assert_eq!(index.visible_lines, reference.len());
            assert!(index.spans.len() <= folds.len());
            for (row, line) in reference.iter().copied().enumerate() {
                assert_eq!(index.line_for_row(row), Some(line));
                assert_eq!(index.row_for_line(line), Some(row));
            }
            for line in 0..50 {
                assert_eq!(
                    index.row_for_line(line),
                    reference.iter().position(|value| *value == line)
                );
            }
            assert_eq!(index.line_for_row(reference.len()), None);
            assert_eq!(index.row_for_line(50), None);
        }
    }

    #[::core::prelude::v1::test]
    fn hundred_thousand_editor_lines_draw_only_viewport_and_reuse_fold_index() {
        let mut cx = TestAppContext::single();
        let content = "let label = \"日本語 café🙂\";\n".repeat(100_000);
        let (state, window) = document_window(&mut cx, &content);
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            let state_ref = state.read(cx);
            assert!(matches!(
                state_ref.display_line_index(),
                DisplayLineIndex::Unfolded(100_000)
            ));
            assert!(
                state_ref.fold_line_index.is_none(),
                "unfolded view stores no document-sized row vector"
            );
            assert_eq!(state_ref.buffer_line_to_display_row(99_999), Some(99_999));
            assert_eq!(state_ref.display_row_to_buffer_line(99_999), 99_999);
            assert!(state_ref.line_layouts.len() <= 27);
            let started = std::time::Instant::now();
            for _ in 0..20 {
                window.draw(cx).clear();
            }
            eprintln!(
                "100k unfolded editor:20 CPU TestPlatform draws {:?}",
                started.elapsed()
            );
            state.update(cx, |state, cx| {
                state.fold_ranges = vec![FoldRange {
                    start_line: 1,
                    end_line: 99_990,
                }];
                state.toggle_fold_at_line(1, cx);
                assert_eq!(state.display_line_count(), 11);
                assert_eq!(state.buffer_line_to_display_row(2), None);
                assert_eq!(state.display_row_to_buffer_line(2), 99_991);
            });
            let index = state.read(cx).fold_line_index.clone().unwrap();
            assert_eq!(
                index.spans.len(),
                1,
                "fold index stores intervals, not100k rows"
            );
            for _ in 0..20 {
                window.draw(cx).clear();
                let state_ref = state.read(cx);
                assert!(Arc::ptr_eq(
                    &index,
                    state_ref.fold_line_index.as_ref().unwrap()
                ));
                assert!(
                    state_ref.line_layouts.len() <= 11,
                    "hidden-gap shaped layouts are pruned"
                );
            }
            state.update(cx, |state, cx| {
                let byte = state.rope.line_to_byte(50_000);
                state.set_selection_bytes(byte, byte, cx).unwrap();
                assert!(
                    state.folded.is_empty(),
                    "a requested caret reveals its folded line"
                );
                assert!(state.fold_line_index.is_none());
                assert_eq!(state.buffer_line_to_display_row(50_000), Some(50_000));
            });
            window.draw(cx).clear();
            assert!(state.read(cx).line_layouts.len() <= 27);
            assert!(
                state.read(cx).prepared_accessibility_document().is_some(),
                "full text metadata prepared on worker"
            );
        });
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "tree-sitter-rust"))]
    #[::core::prelude::v1::test]
    fn worker_syntax_and_fold_preparation_coalesces_document_revisions() {
        let mut cx = TestAppContext::single();
        let state = cx.new(EditorState::new);
        state.update(&mut cx, |state, cx| {
            state.set_language(Language::Rust);
            for revision in 0..20 {
                let content = (0..1000 + revision).map(|function| format!(
                    "fn revision_{revision}_{function}() {{\n    let label = \"日本語 café🙂\";\n    println!(\"{{label}}\");\n}}\n"
                )).collect::<String>();
                state.set_content(&content, cx);
                assert!(state.reparse_task.is_some());
            }
        });
        cx.run_until_parked();
        state.update(&mut cx, |state, cx| {
            assert!(state.reparse_task.is_none());
            assert_eq!(
                state.syntax_tree.as_ref().unwrap().root_node().end_byte(),
                state.content_len_bytes()
            );
            assert_eq!(
                state.fold_ranges.len(),
                1019,
                "only newest worker's available folds commit"
            );
            state.toggle_fold_at_line(0, cx);
            let index = state.fold_line_index.clone().unwrap();
            assert_eq!(index.spans.len(), 1);
            let caret = state.rope.line_to_byte(1);
            state.set_selection_bytes(caret, caret, cx).unwrap();
            assert!(state.folded.is_empty());
            assert!(state.fold_line_index.is_none());
        });
    }

    struct BlinkHost {
        state: Entity<EditorState>,
        visible: bool,
        _observer: Subscription,
    }
    impl Render for BlinkHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().when(self.visible, |this| {
                this.child(Editor::new(&self.state).w(px(640.0)).h(px(480.0)))
            })
        }
    }

    #[::core::prelude::v1::test]
    fn cursor_blink_stops_on_blur_deactivation_and_retained_hidden_editor() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::init(cx);
            crate::theme::install_theme(cx, Theme::dark());
        });
        let state = cx.new(EditorState::new);
        state.update(&mut cx, |state, cx| {
            state.set_content("caret 日本🙂\n", cx);
            state.set_selection_bytes(0, 0, cx).unwrap();
            assert!(
                state.blink_task.is_none(),
                "unfocused selection does not start a timer"
            );
        });
        let (host, window) = cx.add_window_view({
            let state = state.clone();
            move |_, cx| BlinkHost {
                _observer: cx.observe(&state, |_, _, cx| cx.notify()),
                state,
                visible: true,
            }
        });
        // Test windows are registered active before their activation callback
        // exists; drive an actual deactivate/reactivate platform transition.
        window.deactivate_window();
        window.update(|window, cx| {
            window.activate_window();
            window.focus(&state.focus_handle(cx));
        });
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(
                state.read(cx).blink_task.is_some(),
                "focused={} active={} visible={} reduced={} paint_epoch={}",
                state.read(cx).focus_handle.is_focused(window),
                window.is_window_active(),
                window.is_window_visible(),
                window.reduce_motion(),
                state.read(cx).blink_paint_epoch
            );
        });
        window.run_until_parked();
        window.executor().advance_clock(Duration::from_millis(500));
        window.run_until_parked();
        window.update(|window, cx| {
            assert!(!state.read(cx).cursor_visible, "focused caret still blinks");
            window.draw(cx).clear();
            window.focus(&cx.focus_handle());
        });
        window.run_until_parked();
        window.update(|_, cx| {
            assert!(
                state.read(cx).blink_task.is_none(),
                "blur cancels the task immediately"
            );
            assert!(state.read(cx).cursor_visible);
        });
        window.executor().advance_clock(Duration::from_secs(2));
        window.run_until_parked();
        window.update(|window, cx| {
            assert!(state.read(cx).cursor_visible);
            window.focus(&state.focus_handle(cx));
            window.draw(cx).clear();
            assert!(state.read(cx).blink_task.is_some());
        });
        window.deactivate_window();
        window.update(|_, cx| assert!(state.read(cx).blink_task.is_none()));
        window.update(|window, _| window.activate_window());
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(state.read(cx).blink_task.is_some());
            host.update(cx, |host, cx| {
                host.visible = false;
                cx.notify();
            });
            window.draw(cx).clear();
        });
        window.run_until_parked();
        for _ in 0..3 {
            window.executor().advance_clock(Duration::from_millis(500));
            window.run_until_parked();
        }
        window.update(|_, cx| {
            assert!(
                state.read(cx).blink_task.is_none(),
                "retained hidden state has no recurring timer"
            );
            assert!(state.read(cx).cursor_visible);
        });
    }

    #[::core::prelude::v1::test]
    fn deferred_accessibility_selection_rejects_replaced_document_identity() {
        let mut cx = TestAppContext::single();
        let content = "original café 日本🙂\n";
        let replacement = "modified café 日本🙂\n";
        let (state, window) = document_window(&mut cx, content);
        let start = content.find("日本").unwrap();
        let end = start + "日本🙂".len();
        let (id, original) = window.update(|window, _| {
            let node = window
                .accessibility_tree()
                .nodes
                .values()
                .find(|node| node.label.as_deref() == Some("Project document"))
                .unwrap();
            (node.id, node.text_document.clone().unwrap())
        });
        // This is the normalized payload: the native queue already converted
        // run positions to bytes. Div defers its listener onto the foreground
        // executor, leaving a real edit/reprepare window before invocation.
        let current = window.update(|window, cx| {
            window.dispatch_accessibility_action_for_test(
                AccessibilityActionRequest::with_payload(
                    id,
                    AccessibilityAction::SetTextSelection,
                    AccessibilityActionPayload::TextSelection {
                        document_id: original.id(),
                        anchor: end,
                        focus: start,
                    },
                ),
            );
            let current = state.update(cx, |state, cx| {
                state.set_content(replacement, cx);
                state.prepare_accessibility_document(cx);
                state.set_selection_bytes(1, 1, cx).unwrap();
                let current = state.prepared_accessibility_document().unwrap();
                assert_ne!(current.id(), original.id());
                assert!(
                    current.contains_selection(AccessibilityTextSelection {
                        anchor: end,
                        focus: start
                    }),
                    "same byte endpoints remain valid in the replacement document"
                );
                current
            });
            window.draw(cx).clear();
            current
        });
        window.run_until_parked();
        window.update(|window, cx| {
            assert_eq!(
                state.read(cx).selection_bytes(),
                (1, 1),
                "deferred old-document action is rejected despite freshly prepared current metadata"
            );
            assert_eq!(state.read(cx).content(), replacement);
            window.dispatch_accessibility_action_for_test(
                AccessibilityActionRequest::with_payload(
                    id,
                    AccessibilityAction::SetTextSelection,
                    AccessibilityActionPayload::TextSelection {
                        document_id: current.id(),
                        anchor: end,
                        focus: start,
                    },
                ),
            );
        });
        window.run_until_parked();
        window.update(|_, cx| {
            assert_eq!(state.read(cx).selection_bytes(), (end, start));
            assert_eq!(state.read(cx).selection_text().as_deref(), Some("日本🙂"));
        });
    }

    #[::core::prelude::v1::test]
    fn document_accessibility_selection_uses_complete_revision_and_reuses_metadata() {
        let mut cx = TestAppContext::single();
        let content = "# café 日本🙂\nsecond\n";
        let (state, window) = document_window(&mut cx, content);
        let (id, document) = window.update(|window, cx| {
            let node = window
                .accessibility_tree()
                .nodes
                .values()
                .find(|node| node.label.as_deref() == Some("Project document"))
                .unwrap();
            assert!(
                node.actions
                    .contains(&AccessibilityAction::SetTextSelection)
            );
            assert!(!node.states.contains(AccessibilityState::BUSY));
            assert!(
                node.value.is_none(),
                "native full text is supplied through retained text runs"
            );
            let document = node.text_document.clone().unwrap();
            assert_eq!(document.text(), state.read(cx).content());
            (node.id, document)
        });
        let start = content.find("日本").unwrap();
        let end = start + "日本🙂".len();
        window.update(|window, _| {
            window.dispatch_accessibility_action_for_test(
                AccessibilityActionRequest::with_payload(
                    id,
                    AccessibilityAction::SetTextSelection,
                    AccessibilityActionPayload::TextSelection {
                        document_id: document.id(),
                        anchor: end,
                        focus: start,
                    },
                ),
            );
        });
        window.run_until_parked();
        window.update(|window, cx| {
            assert_eq!(state.read(cx).selection_bytes(), (end, start));
            assert_eq!(state.read(cx).selection_text().as_deref(), Some("日本🙂"));
            for _ in 0..20 {
                state.update(cx, |state, cx| {
                    state.set_selection_bytes(start, start, cx).unwrap()
                });
                window.draw(cx).clear();
                assert!(Arc::ptr_eq(
                    &document,
                    &state.read(cx).prepared_accessibility_document().unwrap()
                ));
            }
            state.update(cx, |state, cx| {
                state.set_content(&"changed 日本🙂\n".repeat(10_000), cx);
                let before = state.selection_bytes();
                assert!(
                    state
                        .set_accessibility_selection(document.id(), end, start, cx)
                        .is_err()
                );
                assert_eq!(
                    state.selection_bytes(),
                    before,
                    "old-revision selection cannot mutate current document"
                );
            });
        });
    }

    #[::core::prelude::v1::test]
    fn large_document_accessibility_preparation_coalesces_revisions_and_reclaims_outgoing_text() {
        let mut cx = TestAppContext::single();
        let state = cx.new(EditorState::new);
        state.update(&mut cx, |state, cx| {
            state.set_content(&"old café🙂\n".repeat(10_000), cx);
            state.prepare_accessibility_document(cx);
            assert!(state.accessibility_preparation_task.is_some());
            for revision in 0..20 {
                state.set_content(&format!("revision {revision} 日本🙂\n").repeat(5_000), cx);
                state.prepare_accessibility_document(cx);
                assert!(state.accessibility_preparation_task.is_some());
                assert!(state.prepared_accessibility_document().is_none());
            }
        });
        cx.run_until_parked();
        let retired = state.update(&mut cx, |state, cx| {
            let document = state.prepared_accessibility_document().unwrap();
            assert_eq!(document.text(), state.content());
            assert!(document.text().starts_with("revision 19 日本🙂"));
            assert!(state.accessibility_preparation_task.is_none());
            let retired = Arc::downgrade(&document);
            state.set_content("small 日本語\n", cx);
            state.prepare_accessibility_document(cx);
            assert_eq!(
                state.prepared_accessibility_document().unwrap().text(),
                "small 日本語\n"
            );
            retired
        });
        cx.run_until_parked();
        assert!(retired.upgrade().is_none());
        state.update(&mut cx, |state, cx| {
            let previous = state.prepared_accessibility_document().unwrap();
            let version = state.content_version();
            state.marked_range = Some(0..1);
            state.load_file(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("examples/fixtures/project_launch.md"),
                cx,
            );
            assert!(
                state.content_version() > version,
                "loading a new file changes document identity"
            );
            assert!(state.marked_range.is_none());
            assert!(state.prepared_accessibility_document().is_none());
            state.prepare_accessibility_document(cx);
            let current = state.prepared_accessibility_document().unwrap();
            assert!(!Arc::ptr_eq(&previous, &current));
            assert_eq!(current.text(), state.content());
        });
    }

    #[::core::prelude::v1::test]
    fn document_selection_bytes_validate_boundaries_and_preserve_history_direction() {
        let mut cx = TestAppContext::single();
        let (state, window) = document_window(&mut cx, "café 日本🙂\nsecond\n");
        window.update(|_, cx| {
            state.update(cx, |state, cx| {
                let before = state.content();
                let start = before.find("日本").unwrap();
                let end = start + "日本🙂".len();
                state.set_selection_bytes(end, start, cx).unwrap();
                assert_eq!(state.selection_bytes(), (end, start));
                assert_eq!(state.selection_text().as_deref(), Some("日本🙂"));
                assert!(state.set_selection_bytes(start + 1, end, cx).is_err());
                assert!(state.set_selection_bytes(0, before.len() + 1, cx).is_err());
                assert_eq!(state.selection_bytes(), (end, start));
                assert_eq!(state.content(), before);
                assert_eq!(state.undo_depth(), 0);
                state.replace_selection("選択", cx);
                assert_eq!(state.undo_depth(), 1);
                let caret = state.selection_bytes().1;
                state.set_selection_bytes(caret, caret, cx).unwrap();
                assert!(state.selection_is_empty());
                assert_eq!(state.undo_depth(), 1);
            })
        });
        window.update(|window, cx| {
            state.update(cx, |state, cx| {
                state.undo(&Undo, window, cx);
                assert_eq!(state.content(), "café 日本🙂\nsecond\n");
            })
        });
    }

    #[::core::prelude::v1::test]
    fn document_ime_replaces_marked_selection_and_commits_as_one_undo_step() {
        let mut cx = TestAppContext::single();
        let (state, window) = document_window(&mut cx, "# Project café\n\nHello 🙂 team\n");
        let original = window.update(|_, cx| state.read(cx).content());
        window.update(|window, cx| {
            state.update(cx, |state, cx| {
                // Select "Hello" backwards: platform ranges stay sorted, with a
                // separate reversed bit, including Unicode before the selection.
                state.selection = Some(Selection::new(Position::new(2, 5), Position::new(2, 0)));
                state.cursor = Position::new(2, 0);
                let selected = state.selected_text_range(false, window, cx).unwrap();
                assert!(selected.reversed);
                assert!(selected.range.start < selected.range.end);
                assert_eq!(
                    state
                        .text_for_range(selected.range, &mut None, window, cx)
                        .as_deref(),
                    Some("Hello")
                );
                state.replace_and_mark_text_in_range(None, "に", Some(1..1), window, cx);
                state.replace_and_mark_text_in_range(None, "日本🙂", Some(2..4), window, cx);
                assert!(state.content().contains("日本🙂 🙂 team"));
                assert!(!state.content().contains("に日本"));
                assert_eq!(state.selection_text().as_deref(), Some("🙂"));
                let marked = state.marked_text_range(window, cx).unwrap();
                assert_eq!(marked.end - marked.start, 4);
                assert_eq!(state.undo_depth(), 1);
                state.replace_text_in_range(None, "日本語", window, cx);
                assert!(state.marked_text_range(window, cx).is_none());
                assert!(state.content().contains("日本語 🙂 team"));
                assert_eq!(state.undo_depth(), 1);
                state.undo(&Undo, window, cx);
                assert_eq!(state.content(), original);
                state.redo(&Redo, window, cx);
                assert!(state.content().contains("日本語 🙂 team"));
                assert_eq!(state.undo_depth(), 1);
            })
        });
    }

    #[::core::prelude::v1::test]
    fn document_input_formatting_unicode_selection_and_clipboard_roundtrip() {
        let mut cx = TestAppContext::single();
        let (state, window) = document_window(&mut cx, "# Project plan\n\nLaunch notes\n");
        #[cfg(target_os = "macos")]
        window.simulate_keystrokes("cmd-a");
        #[cfg(not(target_os = "macos"))]
        window.simulate_keystrokes("ctrl-a");
        window.simulate_input("A e\u{301}👩\u{200d}💻 B\n");
        window.update(|window, cx| {
            state.update(cx, |state, cx| {
                state.set_cursor_position(0, "A e\u{301}👩\u{200d}💻".len(), cx);
                state.select_left(&SelectLeft, window, cx);
                assert_eq!(state.selection_text().as_deref(), Some("👩\u{200d}💻"));
                state.copy(&Copy, window, cx);
                assert_eq!(
                    cx.read_from_clipboard().unwrap().unwrap().text().as_deref(),
                    Some("👩\u{200d}💻")
                );
                state.replace_selection("**developer**", cx);
                assert!(state.content().contains("A e\u{301}**developer** B"));
                state.undo(&Undo, window, cx);
                assert!(state.content().contains("A e\u{301}👩\u{200d}💻 B"));
                state.redo(&Redo, window, cx);
                assert!(state.content().contains("**developer**"));
                state.undo(&Undo, window, cx);
                state.set_cursor_position(0, "A e\u{301}".len(), cx);
                state.backspace(&Backspace, window, cx);
                assert!(state.content().starts_with("A 👩\u{200d}💻"));
                state.undo(&Undo, window, cx);
                state.set_cursor_position(0, 2, cx);
                state.delete(&Delete, window, cx);
                assert!(state.content().starts_with("A 👩\u{200d}💻"));
                state.undo(&Undo, window, cx);
                state.set_cursor_position(0, 2, cx);
                state.move_right(&MoveRight, window, cx);
                assert_eq!(state.cursor.col, "A e\u{301}".len());
                state.move_right(&MoveRight, window, cx);
                assert_eq!(state.cursor.col, "A e\u{301}👩\u{200d}💻".len());
            })
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
    }

    #[::core::prelude::v1::test]
    fn document_explicit_ime_range_moves_to_requested_caret_and_read_only_rejects_edits() {
        let mut cx = TestAppContext::single();
        let (state, window) = document_window(&mut cx, "café 🙂\n");
        window.update(|window, cx| {
            state.update(cx, |state, cx| {
                state.set_cursor_position(0, 0, cx);
                state.replace_and_mark_text_in_range(
                    Some(5..7),
                    "👩\u{200d}💻",
                    Some(5..5),
                    window,
                    cx,
                );
                assert_eq!(state.content(), "café 👩\u{200d}💻\n");
                assert_eq!(state.cursor.col, "café 👩\u{200d}💻".len());
                state.unmark_text(window, cx);
                let content = state.content();
                let depth = state.undo_depth();
                state.read_only = true;
                state.replace_and_mark_text_in_range(None, "blocked", None, window, cx);
                state.replace_text_in_range(None, "blocked", window, cx);
                state.replace_selection("blocked", cx);
                state.undo(&Undo, window, cx);
                state.redo(&Redo, window, cx);
                assert_eq!(state.content(), content);
                assert_eq!(state.undo_depth(), depth);
            })
        });
    }
}

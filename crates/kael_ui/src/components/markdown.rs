//! ASTRYX Markdown facade backed by the Kael UI display implementation.

pub use crate::display::markdown::{
    createIncrementalState, create_incremental_state, parseInline, parseMarkdown,
    parseMarkdownIncremental, parse_inline, parse_markdown, parse_markdown_incremental, BlockNode,
    IncrementalParseState, IncrementalState, InlineNode, ListItemNode, Markdown,
    MarkdownComponents, MarkdownInlinePlugin, MarkdownSource, TableCellNode,
};

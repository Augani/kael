//! Typed, presentation-neutral Markdown parsing for Kael applications.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Horizontal alignment declared for one Markdown table column.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableAlignment {
    /// Align cell content to the leading edge.
    Left,
    /// Center cell content.
    Center,
    /// Align cell content to the trailing edge.
    Right,
}

/// An inline Markdown node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InlineNode {
    /// Plain text.
    Text(String),
    /// Strong emphasis.
    Bold(Vec<InlineNode>),
    /// Emphasis.
    Italic(Vec<InlineNode>),
    /// Struck-through content.
    Strikethrough(Vec<InlineNode>),
    /// Inline code.
    Code(String),
    /// A link and its destination.
    Link {
        /// Visible link content.
        text: Vec<InlineNode>,
        /// Unvalidated link destination. The application must apply its URL policy.
        url: String,
    },
    /// An inline image reference.
    Image {
        /// Alternative text.
        alt: String,
        /// Unvalidated image source. The application must apply its fetch policy.
        url: String,
    },
    /// An explicit line break.
    LineBreak,
    /// Raw inline HTML. Parsers preserve it as inert source; applications must not execute it
    /// without a separate sanitization and permission policy.
    Html(String),
}

/// One item in an ordered, unordered, or task list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListItemNode {
    /// Task-list state, or `None` for a normal list item.
    pub checked: Option<bool>,
    /// Block content owned by this item.
    ///
    /// Nested lists remain [`BlockNode::OrderedList`] or [`BlockNode::UnorderedList`]
    /// entries, preserving their kind, ordered-list start value, and full item content.
    pub blocks: Vec<BlockNode>,
}

/// A block-level Markdown node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BlockNode {
    /// A paragraph.
    Paragraph(Vec<InlineNode>),
    /// A heading.
    Heading {
        /// Heading level in the range 1 through 6.
        level: u8,
        /// Heading content.
        content: Vec<InlineNode>,
    },
    /// A fenced or indented code block.
    CodeBlock {
        /// Optional fenced language tag.
        language: Option<String>,
        /// Literal code content.
        code: String,
    },
    /// A block quote containing other blocks.
    BlockQuote(Vec<BlockNode>),
    /// An ordered list.
    OrderedList {
        /// First visible list number.
        start: u64,
        /// List items.
        items: Vec<ListItemNode>,
    },
    /// An unordered list.
    UnorderedList {
        /// List items.
        items: Vec<ListItemNode>,
    },
    /// A table.
    Table {
        /// Header cells.
        headers: Vec<Vec<InlineNode>>,
        /// Per-column alignment.
        alignments: Vec<TableAlignment>,
        /// Body rows and cells.
        rows: Vec<Vec<Vec<InlineNode>>>,
    },
    /// A horizontal rule.
    HorizontalRule,
    /// A block image reference.
    Image {
        /// Alternative text.
        alt: String,
        /// Unvalidated image source.
        url: String,
    },
}

/// Cached incremental parse state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IncrementalParseState {
    previous_source: String,
    blocks: Vec<BlockNode>,
}

impl IncrementalParseState {
    /// Returns the most recently parsed source without reparsing it.
    pub fn previous_source(&self) -> &str {
        &self.previous_source
    }

    /// Returns the most recently parsed blocks.
    pub fn blocks(&self) -> &[BlockNode] {
        &self.blocks
    }
}

/// Parses CommonMark plus tables, strikethrough, and task lists into typed nodes.
pub fn parse_markdown(source: &str) -> Vec<BlockNode> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    let events = Parser::new_ext(source, options).collect::<Vec<_>>();
    EventCursor::new(&events).parse_document()
}

/// Parses a Markdown fragment and returns its inline-capable content.
pub fn parse_inline(source: &str) -> Vec<InlineNode> {
    parse_markdown(source)
        .into_iter()
        .flat_map(|block| match block {
            BlockNode::Paragraph(inlines) => inlines,
            BlockNode::Heading { content, .. } => content,
            BlockNode::CodeBlock { code, .. } => vec![InlineNode::Code(code)],
            BlockNode::Image { alt, url } => vec![InlineNode::Image { alt, url }],
            BlockNode::BlockQuote(_)
            | BlockNode::OrderedList { .. }
            | BlockNode::UnorderedList { .. }
            | BlockNode::Table { .. }
            | BlockNode::HorizontalRule => Vec::new(),
        })
        .collect()
}

/// Reparses only when the source changed and returns a clone of the cached typed nodes.
pub fn parse_markdown_incremental(
    state: &mut IncrementalParseState,
    source: &str,
) -> Vec<BlockNode> {
    if state.previous_source != source {
        state.previous_source.clear();
        state.previous_source.push_str(source);
        state.blocks = parse_markdown(source);
    }
    state.blocks.clone()
}

struct EventCursor<'events, 'source> {
    events: &'events [Event<'source>],
    position: usize,
}

impl<'events, 'source> EventCursor<'events, 'source> {
    fn new(events: &'events [Event<'source>]) -> Self {
        Self {
            events,
            position: 0,
        }
    }

    fn parse_document(&mut self) -> Vec<BlockNode> {
        let mut ignored_task_marker = None;
        self.parse_blocks_until(None, &mut ignored_task_marker)
    }

    fn parse_blocks_until(
        &mut self,
        expected_end: Option<TagEnd>,
        task_marker: &mut Option<bool>,
    ) -> Vec<BlockNode> {
        let mut blocks = Vec::new();

        while let Some(event) = self.next() {
            match event {
                Event::End(end) if Some(end) == expected_end => break,
                Event::End(_) => {}
                Event::Start(Tag::Paragraph) => {
                    let inlines = self.parse_inlines_until(TagEnd::Paragraph, task_marker);
                    blocks.push(paragraph_or_image(inlines));
                }
                Event::Start(Tag::Heading { level, .. }) => {
                    let content = self.parse_inlines_until(TagEnd::Heading(level), task_marker);
                    blocks.push(BlockNode::Heading {
                        level: heading_level(level),
                        content,
                    });
                }
                Event::Start(Tag::BlockQuote(kind)) => {
                    let mut ignored_nested_task_marker = None;
                    let quoted = self.parse_blocks_until(
                        Some(TagEnd::BlockQuote(kind)),
                        &mut ignored_nested_task_marker,
                    );
                    blocks.push(BlockNode::BlockQuote(quoted));
                }
                Event::Start(Tag::CodeBlock(kind)) => {
                    blocks.push(self.parse_code_block(kind));
                }
                Event::Start(Tag::HtmlBlock) => {
                    blocks.push(self.parse_html_block());
                }
                Event::Start(Tag::List(start)) => blocks.push(self.parse_list(start)),
                Event::Start(Tag::Table(alignments)) => {
                    blocks.push(self.parse_table(&alignments));
                }
                Event::Start(tag) => {
                    if is_inline_tag(&tag) {
                        self.position -= 1;
                        blocks.push(paragraph_or_image(self.parse_inline_run(task_marker)));
                    } else {
                        let mut ignored_nested_task_marker = None;
                        blocks.extend(self.parse_blocks_until(
                            Some(tag.to_end()),
                            &mut ignored_nested_task_marker,
                        ));
                    }
                }
                Event::Rule => blocks.push(BlockNode::HorizontalRule),
                Event::Text(_)
                | Event::Code(_)
                | Event::InlineMath(_)
                | Event::DisplayMath(_)
                | Event::Html(_)
                | Event::InlineHtml(_)
                | Event::FootnoteReference(_)
                | Event::SoftBreak
                | Event::HardBreak
                | Event::TaskListMarker(_) => {
                    self.position -= 1;
                    blocks.push(paragraph_or_image(self.parse_inline_run(task_marker)));
                }
            }
        }

        blocks
    }

    fn parse_code_block(&mut self, kind: pulldown_cmark::CodeBlockKind<'source>) -> BlockNode {
        let language = match kind {
            pulldown_cmark::CodeBlockKind::Fenced(language) => {
                let language = language.trim().to_owned();
                (!language.is_empty()).then_some(language)
            }
            pulldown_cmark::CodeBlockKind::Indented => None,
        };
        let mut code = String::new();

        while let Some(event) = self.next() {
            match event {
                Event::End(TagEnd::CodeBlock) => break,
                Event::Text(text)
                | Event::Code(text)
                | Event::InlineMath(text)
                | Event::DisplayMath(text)
                | Event::Html(text)
                | Event::InlineHtml(text) => code.push_str(&text),
                Event::SoftBreak | Event::HardBreak => code.push('\n'),
                Event::FootnoteReference(label) => {
                    code.push_str("[^");
                    code.push_str(&label);
                    code.push(']');
                }
                Event::TaskListMarker(checked) => {
                    code.push_str(if checked { "[x]" } else { "[ ]" });
                }
                Event::Rule | Event::Start(_) | Event::End(_) => {}
            }
        }

        BlockNode::CodeBlock { language, code }
    }

    fn parse_html_block(&mut self) -> BlockNode {
        let mut source = String::new();
        while let Some(event) = self.next() {
            match event {
                Event::End(TagEnd::HtmlBlock) => break,
                Event::Html(html) | Event::InlineHtml(html) | Event::Text(html) => {
                    source.push_str(&html);
                }
                Event::SoftBreak | Event::HardBreak => source.push('\n'),
                _ => {}
            }
        }
        BlockNode::Paragraph(vec![InlineNode::Html(source)])
    }

    fn parse_list(&mut self, start: Option<u64>) -> BlockNode {
        let mut items = Vec::new();
        let expected_end = TagEnd::List(start.is_some());

        while let Some(event) = self.next() {
            match event {
                Event::Start(Tag::Item) => items.push(self.parse_list_item()),
                Event::End(end) if end == expected_end => break,
                Event::Start(tag) => self.skip_until(tag.to_end()),
                _ => {}
            }
        }

        if let Some(start) = start {
            BlockNode::OrderedList { start, items }
        } else {
            BlockNode::UnorderedList { items }
        }
    }

    fn parse_list_item(&mut self) -> ListItemNode {
        let mut checked = None;
        let blocks = self.parse_blocks_until(Some(TagEnd::Item), &mut checked);
        ListItemNode { checked, blocks }
    }

    fn parse_table(&mut self, alignments: &[pulldown_cmark::Alignment]) -> BlockNode {
        let alignments = alignments
            .iter()
            .map(|alignment| match alignment {
                pulldown_cmark::Alignment::Left | pulldown_cmark::Alignment::None => {
                    TableAlignment::Left
                }
                pulldown_cmark::Alignment::Center => TableAlignment::Center,
                pulldown_cmark::Alignment::Right => TableAlignment::Right,
            })
            .collect();
        let mut headers = Vec::new();
        let mut rows = Vec::new();

        while let Some(event) = self.next() {
            match event {
                Event::Start(Tag::TableHead) => headers = self.parse_table_head(),
                Event::Start(Tag::TableRow) => rows.push(self.parse_table_row()),
                Event::End(TagEnd::Table) => break,
                Event::Start(tag) => self.skip_until(tag.to_end()),
                _ => {}
            }
        }

        BlockNode::Table {
            headers,
            alignments,
            rows,
        }
    }

    fn parse_table_head(&mut self) -> Vec<Vec<InlineNode>> {
        let mut headers = Vec::new();

        while let Some(event) = self.next() {
            match event {
                Event::Start(Tag::TableCell) => headers.push(self.parse_table_cell()),
                Event::Start(Tag::TableRow) => headers = self.parse_table_row(),
                Event::End(TagEnd::TableHead) => break,
                Event::Start(tag) => self.skip_until(tag.to_end()),
                _ => {}
            }
        }

        headers
    }

    fn parse_table_row(&mut self) -> Vec<Vec<InlineNode>> {
        let mut row = Vec::new();

        while let Some(event) = self.next() {
            match event {
                Event::Start(Tag::TableCell) => row.push(self.parse_table_cell()),
                Event::End(TagEnd::TableRow) => break,
                Event::Start(tag) => self.skip_until(tag.to_end()),
                _ => {}
            }
        }

        row
    }

    fn parse_table_cell(&mut self) -> Vec<InlineNode> {
        let mut ignored_task_marker = None;
        self.parse_inlines_until(TagEnd::TableCell, &mut ignored_task_marker)
    }

    fn parse_inlines_until(
        &mut self,
        expected_end: TagEnd,
        task_marker: &mut Option<bool>,
    ) -> Vec<InlineNode> {
        let mut inlines = Vec::new();

        while let Some(event) = self.next() {
            if matches!(event, Event::End(end) if end == expected_end) {
                break;
            }
            self.append_inline_event(event, task_marker, &mut inlines);
        }

        inlines
    }

    fn parse_inline_run(&mut self, task_marker: &mut Option<bool>) -> Vec<InlineNode> {
        let mut inlines = Vec::new();
        while self.events.get(self.position).is_some_and(is_inline_event) {
            let event = self
                .next()
                .expect("the inline event was checked before advancing");
            self.append_inline_event(event, task_marker, &mut inlines);
        }
        inlines
    }

    fn append_inline_event(
        &mut self,
        event: Event<'source>,
        task_marker: &mut Option<bool>,
        inlines: &mut Vec<InlineNode>,
    ) {
        match event {
            Event::Start(Tag::Strong) => inlines.push(InlineNode::Bold(
                self.parse_inlines_until(TagEnd::Strong, task_marker),
            )),
            Event::Start(Tag::Emphasis) => inlines.push(InlineNode::Italic(
                self.parse_inlines_until(TagEnd::Emphasis, task_marker),
            )),
            Event::Start(Tag::Strikethrough) => inlines.push(InlineNode::Strikethrough(
                self.parse_inlines_until(TagEnd::Strikethrough, task_marker),
            )),
            Event::Start(Tag::Link { dest_url, .. }) => {
                let text = self.parse_inlines_until(TagEnd::Link, task_marker);
                inlines.push(InlineNode::Link {
                    text,
                    url: dest_url.to_string(),
                });
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                let alt = plain_text(&self.parse_inlines_until(TagEnd::Image, task_marker));
                inlines.push(InlineNode::Image {
                    alt,
                    url: dest_url.to_string(),
                });
            }
            Event::Start(tag) => {
                inlines.extend(self.parse_inlines_until(tag.to_end(), task_marker));
            }
            Event::End(_) | Event::Rule => {}
            Event::Text(text) => inlines.push(InlineNode::Text(text.to_string())),
            Event::Code(code) | Event::InlineMath(code) | Event::DisplayMath(code) => {
                inlines.push(InlineNode::Code(code.to_string()));
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                inlines.push(InlineNode::Html(html.to_string()));
            }
            Event::FootnoteReference(label) => {
                inlines.push(InlineNode::Text(format!("[^{label}]")));
            }
            Event::SoftBreak => inlines.push(InlineNode::Text(" ".into())),
            Event::HardBreak => inlines.push(InlineNode::LineBreak),
            Event::TaskListMarker(checked) => *task_marker = Some(checked),
        }
    }

    fn skip_until(&mut self, expected_end: TagEnd) {
        let mut depth = 0usize;
        while let Some(event) = self.next() {
            match event {
                Event::Start(_) => depth += 1,
                Event::End(end) if depth == 0 && end == expected_end => break,
                Event::End(_) => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }

    fn next(&mut self) -> Option<Event<'source>> {
        let event = self.events.get(self.position)?.clone();
        self.position += 1;
        Some(event)
    }
}

fn is_inline_tag(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. } | Tag::Image { .. }
    )
}

fn is_inline_event(event: &Event<'_>) -> bool {
    match event {
        Event::Start(tag) => is_inline_tag(tag),
        Event::Text(_)
        | Event::Code(_)
        | Event::InlineMath(_)
        | Event::DisplayMath(_)
        | Event::Html(_)
        | Event::InlineHtml(_)
        | Event::FootnoteReference(_)
        | Event::SoftBreak
        | Event::HardBreak
        | Event::TaskListMarker(_) => true,
        Event::End(_) | Event::Rule => false,
    }
}

fn paragraph_or_image(inlines: Vec<InlineNode>) -> BlockNode {
    if let [InlineNode::Image { alt, url }] = inlines.as_slice() {
        BlockNode::Image {
            alt: alt.clone(),
            url: url.clone(),
        }
    } else {
        BlockNode::Paragraph(inlines)
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn plain_text(inlines: &[InlineNode]) -> String {
    let mut text = String::new();
    for inline in inlines {
        match inline {
            InlineNode::Text(value) | InlineNode::Code(value) | InlineNode::Html(value) => {
                text.push_str(value);
            }
            InlineNode::Bold(children)
            | InlineNode::Italic(children)
            | InlineNode::Strikethrough(children) => text.push_str(&plain_text(children)),
            InlineNode::Link { text: children, .. } => text.push_str(&plain_text(children)),
            InlineNode::Image { alt, .. } => text.push_str(alt),
            InlineNode::LineBreak => text.push('\n'),
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_github_collaboration_markdown() {
        let blocks = parse_markdown(
            "# Review\n\n- [x] Ready\n- Parent\n  - Child\n\n```rust\nfn main() {}\n```\n\n| A | B |\n| :- | -: |\n| 1 | 2 |",
        );
        assert!(matches!(
            blocks.first(),
            Some(BlockNode::Heading { level: 1, .. })
        ));
        assert!(blocks.iter().any(|block| matches!(
            block,
            BlockNode::UnorderedList { items }
                if items.first().is_some_and(|item| item.checked == Some(true))
                    && items.get(1).is_some_and(|item| item.blocks.iter().any(|block| {
                        matches!(block, BlockNode::UnorderedList { items } if items.len() == 1)
                    }))
        )));
        assert!(blocks.iter().any(|block| matches!(
            block,
            BlockNode::CodeBlock { language: Some(language), .. } if language == "rust"
        )));
        assert!(blocks.iter().any(|block| matches!(
            block,
            BlockNode::Table { alignments, .. }
                if alignments == &[TableAlignment::Left, TableAlignment::Right]
        )));
    }

    #[test]
    fn incremental_parse_reuses_unchanged_source() {
        let mut state = IncrementalParseState::default();
        let first = parse_markdown_incremental(&mut state, "**one**");
        let repeated = parse_markdown_incremental(&mut state, "**one**");
        assert_eq!(first, repeated);
        let changed = parse_markdown_incremental(&mut state, "*two*");
        assert_ne!(first, changed);
        assert_eq!(state.previous_source(), "*two*");
    }

    #[test]
    fn raw_html_is_preserved_as_inert_source() {
        let blocks = parse_markdown("<script>alert(1)</script>");
        assert!(blocks.iter().any(|block| matches!(
            block,
            BlockNode::Paragraph(inlines)
                if inlines.iter().any(|inline| matches!(inline, InlineNode::Html(source) if source.contains("script")))
        )));
    }

    #[test]
    fn inline_image_preserves_surrounding_content_order() {
        assert_eq!(
            parse_markdown("before ![alt](image.png) after"),
            vec![BlockNode::Paragraph(vec![
                InlineNode::Text("before ".into()),
                InlineNode::Image {
                    alt: "alt".into(),
                    url: "image.png".into(),
                },
                InlineNode::Text(" after".into()),
            ])]
        );
    }

    #[test]
    fn linked_image_remains_inside_its_link() {
        assert_eq!(
            parse_markdown("before [![alt](image.png)](https://example.test) after"),
            vec![BlockNode::Paragraph(vec![
                InlineNode::Text("before ".into()),
                InlineNode::Link {
                    text: vec![InlineNode::Image {
                        alt: "alt".into(),
                        url: "image.png".into(),
                    }],
                    url: "https://example.test".into(),
                },
                InlineNode::Text(" after".into()),
            ])]
        );
    }

    #[test]
    fn standalone_image_is_a_block_and_remains_inline_capable() {
        let image = InlineNode::Image {
            alt: "diagram".into(),
            url: "diagram.png".into(),
        };
        assert_eq!(
            parse_markdown("![diagram](diagram.png)"),
            vec![BlockNode::Image {
                alt: "diagram".into(),
                url: "diagram.png".into(),
            }]
        );
        assert_eq!(parse_inline("![diagram](diagram.png)"), vec![image]);
    }

    #[test]
    fn images_preserve_order_in_list_items_and_table_cells() {
        let blocks = parse_markdown(
            "- before ![list](list.png) after\n\n| Column |\n| --- |\n| before ![cell](cell.png) after |",
        );

        let BlockNode::UnorderedList { items } = &blocks[0] else {
            panic!("expected an unordered list");
        };
        assert_eq!(
            items[0].blocks,
            vec![BlockNode::Paragraph(vec![
                InlineNode::Text("before ".into()),
                InlineNode::Image {
                    alt: "list".into(),
                    url: "list.png".into(),
                },
                InlineNode::Text(" after".into()),
            ])]
        );

        let BlockNode::Table { rows, .. } = &blocks[1] else {
            panic!("expected a table");
        };
        assert_eq!(
            rows[0][0],
            vec![
                InlineNode::Text("before ".into()),
                InlineNode::Image {
                    alt: "cell".into(),
                    url: "cell.png".into(),
                },
                InlineNode::Text(" after".into()),
            ]
        );
    }

    #[test]
    fn nested_lists_preserve_kind_start_and_full_block_content() {
        let blocks = parse_markdown(
            "3. parent first paragraph\n\n   parent second paragraph\n\n   - unordered child\n\n     7. ordered grandchild\n\n   > quoted\n\n   ```rust\n   fn nested() {}\n   ```",
        );

        let BlockNode::OrderedList { start, items } = &blocks[0] else {
            panic!("expected an ordered list");
        };
        assert_eq!(*start, 3);
        let parent = &items[0];
        assert_eq!(parent.blocks.len(), 5);
        assert!(matches!(
            &parent.blocks[0],
            BlockNode::Paragraph(content)
                if content == &[InlineNode::Text("parent first paragraph".into())]
        ));
        assert!(matches!(
            &parent.blocks[1],
            BlockNode::Paragraph(content)
                if content == &[InlineNode::Text("parent second paragraph".into())]
        ));

        let BlockNode::UnorderedList { items } = &parent.blocks[2] else {
            panic!("expected a nested unordered list");
        };
        assert!(
            items[0].blocks.len() > 1,
            "expected the ordered list inside the unordered item: {:#?}",
            items[0].blocks
        );
        let BlockNode::OrderedList { start, items } = &items[0].blocks[1] else {
            panic!("expected a nested ordered list");
        };
        assert_eq!(*start, 7);
        assert!(matches!(
            &items[0].blocks[0],
            BlockNode::Paragraph(content)
                if content == &[InlineNode::Text("ordered grandchild".into())]
        ));
        assert!(matches!(
            &parent.blocks[3],
            BlockNode::BlockQuote(blocks)
                if matches!(blocks.as_slice(), [BlockNode::Paragraph(content)]
                    if content == &[InlineNode::Text("quoted".into())])
        ));
        assert!(matches!(
            &parent.blocks[4],
            BlockNode::CodeBlock {
                language: Some(language),
                code,
            } if language == "rust" && code == "fn nested() {}\n"
        ));
    }
}

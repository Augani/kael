# kael_markdown

Presentation-neutral Markdown parsing for Kael applications.

The crate converts CommonMark plus tables, strikethrough, and task lists into a
small typed block/inline tree. Applications keep ownership of typography,
colors, link permissions, image loading, code highlighting, layout, and scroll
behavior. This makes the parser safe to reuse without depending on `kael_ui`'s
theme or component graph.

List items retain their complete block content, so multiple paragraphs, block
quotes, code blocks, and nested ordered or unordered lists can be rendered
without losing structure. Images remain inline when surrounded by other content;
a paragraph containing only an image is represented as `BlockNode::Image`.

```rust
use kael_markdown::{BlockNode, parse_markdown};

let document = parse_markdown("# Review\n\n- [x] Ready");
assert!(matches!(document.first(), Some(BlockNode::Heading { .. })));
```

# Atlas desktop launch plan

**Owner:** Product team · **Review:** 2 October 2026

Atlas helps designers, engineers and support teams work together in one desktop workspace. This draft combines the rollout plan, release criteria and customer feedback.

## What we are shipping

- A project explorer that loads only the folders you open.
- A workspace whose panes can be rearranged, floated and restored.
- Editable records with stable identity, frozen columns and keyboard navigation.
- A document editor that preserves selection and undo history.

### Release checklist

- [x] Verify a realistic project with 100,000 files.
- [x] Exercise edits, clipboard round trips and source cancellation.
- [ ] Review the workflow with native assistive technology.
- [ ] Verify Japanese composition, emoji and accented customer names on real keyboards.

## Team notes

Café reviews begin at 09:30. The Tokyo team wrote: 日本語の入力も大切です。 Our designer 👩‍💻 asked that movement and selection treat an emoji as one visible character.

Use **Bold**, *Italic* or `Code` to format selected source text. Undo should restore the selection's original content in one step. The rendered preview updates when the document changes.

| Milestone | Owner | Target |
| --- | --- | --- |
| Accessibility review | Samira | 6 October |
| Performance capture | Kojo | 8 October |
| Public preview | Mei | 12 October |

## API sketch

```rust
let files = cx.new(|cx| FileTreeState::filesystem(project_root, cx).unwrap());
VirtualFileTree::new("project", files).h(px(480.0));
```

> A release is ready when the interactions people rely on have been exercised in a real application.

The record grid on the right loads from a loopback HTTP fixture. It keeps 100,000 records logical, sends only requested tiles, and saves edits to the fixture. Changing sort cancels pending loads and replaces row identity. Refresh reloads the same records.

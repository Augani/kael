# Workspace Template

An editor/workspace template with a file explorer, syntax-aware editor, toolbar, and status bar.

## Structure

- Keyboard-accessible file explorer with in-memory sample files
- Syntax-aware editor that updates when a file is selected
- Working Undo and Redo controls; unavailable demo actions are clearly disabled
- Status bar that tracks the selected file's language

## Running

```bash
cargo run
```

If the full Xcode Metal toolchain is unavailable on macOS:

```bash
cargo run --features kael/runtime_shaders
```

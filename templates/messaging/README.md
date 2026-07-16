# Messaging App Template

A polished messaging app template with a conversation sidebar, accessible chat thread, and working message composer.

## Structure

- Searchable conversation list with keyboard-selectable rows
- Conversation header and labeled call controls
- Scrollable message history
- Composer that appends sent messages and clears after sending

## Running

```bash
cargo run
```

If the full Xcode Metal toolchain is unavailable on macOS, compile shaders at runtime:

```bash
cargo run --features kael/runtime_shaders
```

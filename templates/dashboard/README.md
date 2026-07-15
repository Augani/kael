# Dashboard Template

A responsive native analytics dashboard with working navigation, charts, search, and an accessible data table.

## Structure

- Keyboard-accessible section navigation with focused content per section
- Responsive metric cards, revenue line chart, and regional bar chart
- Searchable recent-orders table
- Working notification panel and “View all” navigation

## Running

```bash
cargo run
```

If the full Xcode Metal toolchain is unavailable on macOS:

```bash
cargo run --features kael/runtime_shaders
```

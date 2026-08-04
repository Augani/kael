# kael_gpu_budget

Cross-platform GPU memory-budget queries for Kael and standalone Rust apps.
It reads Metal's recommended working set on macOS, DXGI video-memory data on
Windows, and `VK_EXT_memory_budget` on Linux.

```rust
if let Some(budget) = kael_gpu_budget::GpuMemoryBudget::query() {
    println!("{:.0}% GPU memory used", budget.utilization() * 100.0);
}
```

Licensed under Apache-2.0.

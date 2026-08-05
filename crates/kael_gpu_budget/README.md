# kael_gpu_budget

Cross-platform GPU memory-budget queries for Kael and standalone Rust apps.
It reads Metal's recommended working set on macOS, DXGI video-memory data on
Windows, and `VK_EXT_memory_budget` on Linux.

The crate has no UI dependency. Renderers, media pipelines, and other
GPU-intensive systems can use it to size caches or lower quality before the
driver starts evicting resources.

```rust,no_run
if let Some(budget) = kael_gpu_budget::GpuMemoryBudget::query() {
    println!("{:.0}% GPU memory used", budget.utilization() * 100.0);
    println!("{} bytes available", budget.available_bytes());
}
```

`GpuMemoryBudget::query` returns `None` when the native API, a suitable adapter,
or the required Vulkan extension is unavailable. Metal and DXGI use the
platform-selected device. Vulkan has no process-wide default device without a
presentation context, so Linux selects the supported physical device with the
largest reported budget. Reported usage is clamped for ratio calculations
because drivers can transiently report usage above their current budget.

## License

Licensed under the Apache License, Version 2.0. See
[LICENSE-APACHE](LICENSE-APACHE).

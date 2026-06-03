//! Cross-platform GPU memory budget query.
//!
//! Reports the device's memory budget and current usage via the native API on
//! each platform — Metal (`recommendedMaxWorkingSetSize`), DXGI
//! (`IDXGIAdapter3::QueryVideoMemoryInfo`), and Vulkan
//! (`VK_EXT_memory_budget`) — behind one [`GpuMemoryBudget::query`] entry point.

#![deny(missing_docs)]

/// A snapshot of GPU memory budget and usage for the default device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuMemoryBudget {
    /// The device-recommended memory budget, in bytes.
    pub total_bytes: u64,
    /// Bytes currently allocated on the device, in bytes.
    pub used_bytes: u64,
    /// Whether the device shares memory with the CPU (unified memory).
    pub has_unified_memory: bool,
}

impl GpuMemoryBudget {
    /// Bytes still available within the budget.
    pub fn available_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.used_bytes)
    }

    /// Fraction of the budget currently in use, in `0.0..=1.0`.
    pub fn utilization(&self) -> f64 {
        if self.total_bytes == 0 {
            0.0
        } else {
            (self.used_bytes as f64 / self.total_bytes as f64).clamp(0.0, 1.0)
        }
    }

    /// Query the default GPU's budget, or `None` if it is unavailable.
    pub fn query() -> Option<Self> {
        platform_query()
    }
}

#[cfg(target_os = "macos")]
fn platform_query() -> Option<GpuMemoryBudget> {
    let device = metal::Device::system_default()?;
    Some(GpuMemoryBudget {
        total_bytes: device.recommended_max_working_set_size(),
        used_bytes: device.current_allocated_size(),
        has_unified_memory: device.has_unified_memory(),
    })
}

#[cfg(target_os = "windows")]
fn platform_query() -> Option<GpuMemoryBudget> {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO,
        IDXGIAdapter3, IDXGIFactory,
    };
    use windows::core::Interface;

    unsafe {
        let factory: IDXGIFactory = CreateDXGIFactory().ok()?;
        let adapter = factory.EnumAdapters(0).ok()?;
        let adapter3 = adapter.cast::<IDXGIAdapter3>().ok()?;
        let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        adapter3
            .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info)
            .ok()?;
        Some(GpuMemoryBudget {
            total_bytes: info.Budget,
            used_bytes: info.CurrentUsage,
            has_unified_memory: false,
        })
    }
}

#[cfg(target_os = "linux")]
fn platform_query() -> Option<GpuMemoryBudget> {
    use ash::vk;

    unsafe {
        let entry = ash::Entry::load().ok()?;
        let app_info = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
        let create_info = vk::InstanceCreateInfo::default().application_info(&app_info);
        let instance = entry.create_instance(&create_info, None).ok()?;

        let result = (|| {
            let devices = instance.enumerate_physical_devices().ok()?;
            let physical = *devices.first()?;

            let mut budget = vk::PhysicalDeviceMemoryBudgetPropertiesEXT::default();
            let memory_properties = {
                let mut props2 =
                    vk::PhysicalDeviceMemoryProperties2::default().push_next(&mut budget);
                instance.get_physical_device_memory_properties2(physical, &mut props2);
                props2.memory_properties
            };

            let heaps = memory_properties.memory_heap_count as usize;
            let total: u64 = budget.heap_budget[..heaps].iter().sum();
            let used: u64 = budget.heap_usage[..heaps].iter().sum();
            if total == 0 {
                return None;
            }
            let has_unified_memory = memory_properties.memory_heap_count == 1
                && memory_properties.memory_heaps[..heaps]
                    .iter()
                    .all(|heap| heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL));
            Some(GpuMemoryBudget {
                total_bytes: total,
                used_bytes: used,
                has_unified_memory,
            })
        })();

        instance.destroy_instance(None);
        result
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn platform_query() -> Option<GpuMemoryBudget> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_math() {
        let budget = GpuMemoryBudget {
            total_bytes: 2000,
            used_bytes: 500,
            has_unified_memory: true,
        };
        assert_eq!(budget.available_bytes(), 1500);
        assert!((budget.utilization() - 0.25).abs() < 1e-9);
    }

    #[test]
    fn zero_total_utilization_is_zero() {
        let budget = GpuMemoryBudget {
            total_bytes: 0,
            used_bytes: 0,
            has_unified_memory: false,
        };
        assert_eq!(budget.utilization(), 0.0);
        assert_eq!(budget.available_bytes(), 0);
    }

    #[test]
    fn query_is_callable() {
        let _ = GpuMemoryBudget::query();
    }
}

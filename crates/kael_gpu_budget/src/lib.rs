#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

/// A snapshot of GPU memory budget and usage for a platform-selected device.
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

    /// Query a platform-selected GPU budget, or `None` if it is unavailable.
    pub fn query() -> Option<Self> {
        platform_query()
    }
}

#[cfg(target_os = "macos")]
fn platform_query() -> Option<GpuMemoryBudget> {
    let device = metal::Device::system_default()?;
    let total_bytes = device.recommended_max_working_set_size();
    (total_bytes > 0).then(|| GpuMemoryBudget {
        total_bytes,
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

    // SAFETY: All COM interfaces are created by DXGI, output structures are
    // initialized, and their lifetimes remain within this function.
    unsafe {
        let factory: IDXGIFactory = CreateDXGIFactory().ok()?;
        let adapter = factory.EnumAdapters(0).ok()?;
        let description = adapter.GetDesc().ok()?;
        let adapter3 = adapter.cast::<IDXGIAdapter3>().ok()?;
        let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        adapter3
            .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info)
            .ok()?;
        (info.Budget > 0).then(|| GpuMemoryBudget {
            total_bytes: info.Budget,
            used_bytes: info.CurrentUsage,
            has_unified_memory: description.DedicatedVideoMemory == 0,
        })
    }
}

#[cfg(target_os = "linux")]
fn platform_query() -> Option<GpuMemoryBudget> {
    use ash::vk;

    // SAFETY: Vulkan handles are used only with the instance that created them,
    // and the instance is destroyed after all queries complete.
    unsafe {
        let entry = ash::Entry::load().ok()?;
        let app_info = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
        let create_info = vk::InstanceCreateInfo::default().application_info(&app_info);
        let instance = entry.create_instance(&create_info, None).ok()?;

        let physical_devices = match instance.enumerate_physical_devices() {
            Ok(devices) => devices,
            Err(_) => {
                instance.destroy_instance(None);
                return None;
            }
        };
        let result = physical_devices
            .into_iter()
            .filter_map(|physical| {
                let supports_budget = instance
                    .enumerate_device_extension_properties(physical)
                    .ok()?
                    .iter()
                    .any(|extension| {
                        extension.extension_name_as_c_str().ok() == Some(vk::EXT_MEMORY_BUDGET_NAME)
                    });
                if !supports_budget {
                    return None;
                }

                let mut budget = vk::PhysicalDeviceMemoryBudgetPropertiesEXT::default();
                let memory_properties = {
                    let mut props2 =
                        vk::PhysicalDeviceMemoryProperties2::default().push_next(&mut budget);
                    instance.get_physical_device_memory_properties2(physical, &mut props2);
                    props2.memory_properties
                };

                let heap_count = (memory_properties.memory_heap_count as usize)
                    .min(memory_properties.memory_heaps.len())
                    .min(budget.heap_budget.len())
                    .min(budget.heap_usage.len());
                let local_heaps = memory_properties.memory_heaps[..heap_count]
                    .iter()
                    .enumerate()
                    .filter(|(_, heap)| heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL))
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                let total_bytes = local_heaps.iter().fold(0_u64, |total, &index| {
                    total.saturating_add(budget.heap_budget[index])
                });
                if total_bytes == 0 {
                    return None;
                }
                let used_bytes = local_heaps.iter().fold(0_u64, |used, &index| {
                    used.saturating_add(budget.heap_usage[index])
                });

                let memory_type_count = (memory_properties.memory_type_count as usize)
                    .min(memory_properties.memory_types.len());
                let has_unified_memory = local_heaps.iter().all(|&heap_index| {
                    memory_properties.memory_types[..memory_type_count]
                        .iter()
                        .any(|memory_type| {
                            memory_type.heap_index as usize == heap_index
                                && memory_type
                                    .property_flags
                                    .contains(vk::MemoryPropertyFlags::HOST_VISIBLE)
                        })
                });

                Some(GpuMemoryBudget {
                    total_bytes,
                    used_bytes,
                    has_unified_memory,
                })
            })
            .max_by_key(|budget| budget.total_bytes);

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
    fn driver_usage_above_budget_is_safely_clamped() {
        let budget = GpuMemoryBudget {
            total_bytes: 100,
            used_bytes: 150,
            has_unified_memory: false,
        };

        assert_eq!(budget.available_bytes(), 0);
        assert_eq!(budget.utilization(), 1.0);
    }

    #[test]
    fn query_is_callable() {
        let _ = GpuMemoryBudget::query();
    }
}

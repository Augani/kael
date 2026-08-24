use anyhow::{Context, Result, bail};
use util::ResultExt;
use windows::Win32::{
    Foundation::HMODULE,
    Graphics::{
        Direct3D::{
            D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_10_1,
            D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
        },
        Direct3D11::{
            D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_DEBUG,
            D3D11_FEATURE_D3D10_X_HARDWARE_OPTIONS, D3D11_FEATURE_DATA_D3D10_X_HARDWARE_OPTIONS,
            D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
        },
        Dxgi::{
            CreateDXGIFactory2, DXGI_CREATE_FACTORY_DEBUG, DXGI_CREATE_FACTORY_FLAGS,
            DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE, IDXGIAdapter1, IDXGIFactory6,
        },
    },
};

pub(crate) fn try_to_recover_from_device_lost<T>(
    mut f: impl FnMut() -> Result<T>,
    on_success: impl FnOnce(T),
    on_error: impl FnOnce(),
) {
    let result = (0..5).find_map(|i| {
        if i > 0 {
            // Add a small delay before retrying
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        f().log_err()
    });

    if let Some(result) = result {
        on_success(result);
    } else {
        on_error();
    }
}

#[derive(Clone)]
pub(crate) struct DirectXDevices {
    pub(crate) adapter: IDXGIAdapter1,
    pub(crate) dxgi_factory: IDXGIFactory6,
    pub(crate) device: ID3D11Device,
    pub(crate) device_context: ID3D11DeviceContext,
}

impl DirectXDevices {
    pub(crate) fn new() -> Result<Self> {
        let debug_layer_available = check_debug_layer_available();
        let dxgi_factory =
            get_dxgi_factory(debug_layer_available).context("Creating DXGI factory")?;
        let adapter =
            get_adapter(&dxgi_factory, debug_layer_available).context("Getting DXGI adapter")?;
        let (device, device_context) = {
            let mut context: Option<ID3D11DeviceContext> = None;
            let mut feature_level = D3D_FEATURE_LEVEL::default();
            let device = get_device(
                &adapter,
                Some(&mut context),
                Some(&mut feature_level),
                debug_layer_available,
            )
            .context("Creating Direct3D device")?;
            match feature_level {
                D3D_FEATURE_LEVEL_11_1 => {
                    log::info!("Created device with Direct3D 11.1 feature level.")
                }
                D3D_FEATURE_LEVEL_11_0 => {
                    log::info!("Created device with Direct3D 11.0 feature level.")
                }
                D3D_FEATURE_LEVEL_10_1 => {
                    log::info!("Created device with Direct3D 10.1 feature level.")
                }
                other => anyhow::bail!("unsupported Direct3D feature level: {other:?}"),
            }
            let context = context.ok_or_else(|| {
                anyhow::anyhow!("D3D11CreateDevice succeeded without returning a device context")
            })?;
            (device, context)
        };

        Ok(Self {
            adapter,
            dxgi_factory,
            device,
            device_context,
        })
    }
}

fn parse_force_warp(value: Option<&str>) -> Result<bool> {
    match value {
        None | Some("0") => Ok(false),
        Some("1") => Ok(true),
        Some(other) => bail!("KAEL_FORCE_WARP must be exactly `0` or `1`, not {other:?}"),
    }
}

fn force_warp_from_env() -> Result<bool> {
    match std::env::var("KAEL_FORCE_WARP") {
        Ok(value) => parse_force_warp(Some(&value)),
        Err(std::env::VarError::NotPresent) => parse_force_warp(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            bail!("KAEL_FORCE_WARP must contain valid UTF-8 and be exactly `0` or `1`")
        }
    }
}

#[inline]
fn check_debug_layer_available() -> bool {
    #[cfg(debug_assertions)]
    {
        use std::ffi::c_void;
        use windows::{
            Win32::{Graphics::Dxgi::IDXGIInfoQueue, System::LibraryLoader::GetProcAddress},
            core::{HRESULT, Interface, s},
        };

        // DXGIGetDebugInterface1 is a development-time aid and the export is
        // not present on every otherwise-supported Windows installation. A
        // static import prevents Windows from reaching `main` at all when it
        // is absent, even though Kael can render normally without the debug
        // layer. Resolve the optional probe at runtime instead.
        crate::with_dll_library(s!("dxgi.dll"), |dxgi| unsafe {
            let address = GetProcAddress(dxgi, s!("DXGIGetDebugInterface1"))
                .ok_or_else(|| anyhow::anyhow!("DXGIGetDebugInterface1 is unavailable"))?;
            type GetDebugInterface = unsafe extern "system" fn(
                u32,
                *const windows::core::GUID,
                *mut *mut c_void,
            ) -> HRESULT;
            let get_debug_interface: GetDebugInterface = std::mem::transmute(address);
            let mut info_queue = std::ptr::null_mut();
            let result = get_debug_interface(0, &IDXGIInfoQueue::IID, &mut info_queue);
            if result.is_err() {
                anyhow::bail!("DXGI debug interface is unavailable: {result:?}");
            }
            if info_queue.is_null() {
                anyhow::bail!("DXGI debug probe succeeded without returning an interface");
            }
            drop(IDXGIInfoQueue::from_raw(info_queue));
            Ok(())
        })
        .log_err()
        .is_some()
    }
    #[cfg(not(debug_assertions))]
    {
        false
    }
}

#[inline]
fn get_dxgi_factory(debug_layer_available: bool) -> Result<IDXGIFactory6> {
    let factory_flag = if debug_layer_available {
        DXGI_CREATE_FACTORY_DEBUG
    } else {
        #[cfg(debug_assertions)]
        log::warn!(
            "Failed to get DXGI debug interface. DirectX debugging features will be disabled."
        );
        DXGI_CREATE_FACTORY_FLAGS::default()
    };
    unsafe { Ok(CreateDXGIFactory2(factory_flag)?) }
}

#[inline]
fn get_adapter(dxgi_factory: &IDXGIFactory6, debug_layer_available: bool) -> Result<IDXGIAdapter1> {
    if force_warp_from_env()? {
        log::warn!(
            "KAEL_FORCE_WARP=1: selecting the Direct3D WARP software adapter for correctness/liveness proof"
        );
        return get_warp_adapter(dxgi_factory, debug_layer_available);
    }

    let mut adapter_index = 0_u32;
    loop {
        let adapter: IDXGIAdapter1 = match unsafe {
            dxgi_factory
                .EnumAdapterByGpuPreference(adapter_index, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE)
        } {
            Ok(adapter) => adapter,
            Err(error) => {
                log::warn!(
                    "Direct3D hardware adapter enumeration ended at index {adapter_index} ({error}); falling back to WARP"
                );
                return get_warp_adapter(dxgi_factory, debug_layer_available).context(
                    "No compatible hardware adapter was available; creating WARP adapter",
                );
            }
        };
        if let Ok(desc) = unsafe { adapter.GetDesc1() } {
            let gpu_name = String::from_utf16_lossy(&desc.Description)
                .trim_matches(char::from(0))
                .to_string();
            log::info!("Using GPU (high-performance preference): {}", gpu_name);
        }
        // Check to see whether the adapter supports Direct3D 11, but don't
        // create the actual device yet.
        if get_device(&adapter, None, None, debug_layer_available)
            .log_err()
            .is_some()
        {
            return Ok(adapter);
        }
        adapter_index = adapter_index
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("DXGI adapter index exhausted"))?;
    }
}

fn get_warp_adapter(
    dxgi_factory: &IDXGIFactory6,
    debug_layer_available: bool,
) -> Result<IDXGIAdapter1> {
    let adapter = unsafe {
        dxgi_factory
            .EnumWarpAdapter::<IDXGIAdapter1>()
            .context("Enumerating Direct3D WARP adapter")?
    };
    get_device(&adapter, None, None, debug_layer_available)
        .context("Direct3D WARP does not support Kael's required feature level")?;
    if let Ok(desc) = unsafe { adapter.GetDesc1() } {
        let adapter_name = String::from_utf16_lossy(&desc.Description)
            .trim_matches(char::from(0))
            .to_string();
        log::warn!(
            "Using Direct3D software adapter {adapter_name:?}; this is correctness/liveness fallback, not hardware performance evidence"
        );
    }
    Ok(adapter)
}

#[inline]
fn get_device(
    adapter: &IDXGIAdapter1,
    context: Option<*mut Option<ID3D11DeviceContext>>,
    feature_level: Option<*mut D3D_FEATURE_LEVEL>,
    debug_layer_available: bool,
) -> Result<ID3D11Device> {
    let mut device: Option<ID3D11Device> = None;
    let device_flags = if debug_layer_available {
        D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_DEBUG
    } else {
        D3D11_CREATE_DEVICE_BGRA_SUPPORT
    };
    unsafe {
        D3D11CreateDevice(
            adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            device_flags,
            // 4x MSAA is required for Direct3D Feature Level 10.1 or better
            Some(&[
                D3D_FEATURE_LEVEL_11_1,
                D3D_FEATURE_LEVEL_11_0,
                D3D_FEATURE_LEVEL_10_1,
            ]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            feature_level,
            context,
        )?;
    }
    let device = device
        .ok_or_else(|| anyhow::anyhow!("D3D11CreateDevice succeeded without returning a device"))?;
    let mut data = D3D11_FEATURE_DATA_D3D10_X_HARDWARE_OPTIONS::default();
    unsafe {
        device
            .CheckFeatureSupport(
                D3D11_FEATURE_D3D10_X_HARDWARE_OPTIONS,
                &mut data as *mut _ as _,
                std::mem::size_of::<D3D11_FEATURE_DATA_D3D10_X_HARDWARE_OPTIONS>() as u32,
            )
            .context("Checking GPU device feature support")?;
    }
    if data
        .ComputeShaders_Plus_RawAndStructuredBuffers_Via_Shader_4_x
        .as_bool()
    {
        Ok(device)
    } else {
        Err(anyhow::anyhow!(
            "Required feature StructuredBuffer is not supported by GPU/driver"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::parse_force_warp;

    #[test]
    fn force_warp_environment_value_is_strict() {
        assert!(!parse_force_warp(None).unwrap());
        assert!(!parse_force_warp(Some("0")).unwrap());
        assert!(parse_force_warp(Some("1")).unwrap());
        for invalid in ["", "true", "yes", "01", " 1"] {
            assert!(parse_force_warp(Some(invalid)).is_err(), "{invalid:?}");
        }
    }
}

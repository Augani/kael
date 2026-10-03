//! Bounded GPU targets for custom fragment rendering and direct UI composition.
// GTK's software scene renderer retains the portable public API and model
// validation, but has no programmable GPU transport/registry consumer.
#![cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        feature = "webview-wayland-gtk4"
    ),
    allow(dead_code)
)]

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use crate::{DevicePixels, Size, size};

#[derive(Clone, Debug)]
pub(crate) struct RenderTargetPaint {
    pub(crate) opacity: f32,
    pub(crate) corner_radii: crate::Corners<crate::ScaledPixels>,
    pub(crate) rounded_clip_bounds: crate::Bounds<crate::ScaledPixels>,
    pub(crate) rounded_clip_radii: crate::Corners<crate::ScaledPixels>,
    pub(crate) transform: crate::TransformationMatrix,
    pub(crate) color_filter: crate::ColorFilter,
}

impl Default for RenderTargetPaint {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            corner_radii: Default::default(),
            rounded_clip_bounds: Default::default(),
            rounded_clip_radii: Default::default(),
            transform: Default::default(),
            color_filter: crate::ColorFilter::identity(),
        }
    }
}

/// Byte-identical parameters for every backend's associated-color UI pipeline.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct RenderTargetDisplayParams {
    pub(crate) bounds: [f32; 4],
    pub(crate) mask: [f32; 4],
    pub(crate) corners: [f32; 4],
    pub(crate) rounded_clip: [f32; 4],
    pub(crate) rounded_corners: [f32; 4],
    pub(crate) transform: [f32; 4],
    pub(crate) translation: [f32; 2],
    pub(crate) viewport: [f32; 2],
    pub(crate) color_filter: [f32; 4],
    pub(crate) opacity: f32,
    pub(crate) scalar: u32,
    pub(crate) padding: [u32; 2],
}

impl RenderTargetDisplayParams {
    pub(crate) fn new(
        surface: &crate::PaintSurface,
        target: &RenderTarget,
        viewport: Size<DevicePixels>,
    ) -> Self {
        let bounds = |value: crate::Bounds<crate::ScaledPixels>| {
            [
                value.origin.x.0,
                value.origin.y.0,
                value.size.width.0,
                value.size.height.0,
            ]
        };
        let corners = |value: crate::Corners<crate::ScaledPixels>| {
            [
                value.top_left.0,
                value.top_right.0,
                value.bottom_right.0,
                value.bottom_left.0,
            ]
        };
        #[allow(irrefutable_let_patterns)]
        let crate::PaintSurfaceSource::RenderTarget { paint, .. } = &surface.source else {
            unreachable!("GPU display requires a render target")
        };
        Self {
            bounds: bounds(surface.bounds),
            mask: bounds(surface.content_mask.bounds),
            corners: corners(paint.corner_radii),
            rounded_clip: bounds(paint.rounded_clip_bounds),
            rounded_corners: corners(paint.rounded_clip_radii),
            transform: [
                paint.transform.rotation_scale[0][0],
                paint.transform.rotation_scale[0][1],
                paint.transform.rotation_scale[1][0],
                paint.transform.rotation_scale[1][1],
            ],
            translation: paint.transform.translation,
            viewport: [viewport.width.0 as f32, viewport.height.0 as f32],
            color_filter: [
                paint.color_filter.grayscale,
                paint.color_filter.saturate,
                paint.color_filter.brightness,
                paint.color_filter.contrast,
            ],
            opacity: paint.opacity,
            scalar: u32::from(target.descriptor().format == RenderTargetFormat::R8Unorm),
            padding: [0; 2],
        }
    }
}

const MAX_TARGET_DIMENSION: u32 = 16_384;
const MAX_RENDER_TARGETS: usize = 64;
const DEFAULT_TARGET_BUDGET: u64 = 256 * 1024 * 1024;
static NEXT_RENDERER_OWNER: AtomicU64 = AtomicU64::new(1);

/// Shared admission ledger for every user-owned texture and buffer on a window.
#[derive(Clone)]
pub(crate) struct DeviceBudget(Arc<Mutex<DeviceBudgetState>>, Arc<AtomicBool>);
struct DeviceBudgetState {
    owner: u64,
    limit: u64,
    bytes: u64,
    count: usize,
}
impl Default for DeviceBudget {
    fn default() -> Self {
        let owner = NEXT_RENDERER_OWNER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |owner| {
                owner.checked_add(1)
            })
            .expect("renderer ownership identifiers exhausted");
        Self(
            Arc::new(Mutex::new(DeviceBudgetState {
                owner,
                limit: DEFAULT_TARGET_BUDGET,
                bytes: 0,
                count: 0,
            })),
            Arc::new(AtomicBool::new(true)),
        )
    }
}
impl DeviceBudget {
    pub(crate) fn validity(&self) -> Arc<AtomicBool> {
        self.1.clone()
    }
    pub(crate) fn invalidate(&self) {
        self.1.store(false, Ordering::Release);
    }
    fn state(&self) -> std::sync::MutexGuard<'_, DeviceBudgetState> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub(crate) fn owner(&self) -> u64 {
        self.state().owner
    }
    pub(crate) fn check_request(&self, bytes: u64) -> Result<(), RenderTargetError> {
        if !self.1.load(Ordering::Acquire) {
            return Err(RenderTargetError::WrongDevice);
        }
        if bytes > DEFAULT_TARGET_BUDGET || bytes > self.state().limit {
            return Err(RenderTargetError::BudgetExceeded);
        }
        Ok(())
    }
    pub(crate) fn check_allocation(&self, bytes: u64) -> Result<(), RenderTargetError> {
        self.check_request(bytes)?;
        let state = self.state();
        if state
            .bytes
            .checked_add(bytes)
            .is_none_or(|sum| sum > state.limit)
        {
            return Err(RenderTargetError::BudgetExceeded);
        }
        if state.count >= MAX_RENDER_TARGETS {
            return Err(RenderTargetError::ResourceLimit);
        }
        Ok(())
    }
    pub(crate) fn reserve(&self, bytes: u64) -> Result<(), RenderTargetError> {
        if !self.1.load(Ordering::Acquire) {
            return Err(RenderTargetError::WrongDevice);
        }
        let mut state = self.state();
        if bytes > DEFAULT_TARGET_BUDGET
            || state
                .bytes
                .checked_add(bytes)
                .is_none_or(|sum| sum > state.limit)
        {
            return Err(RenderTargetError::BudgetExceeded);
        }
        if state.count >= MAX_RENDER_TARGETS {
            return Err(RenderTargetError::ResourceLimit);
        }
        state.bytes += bytes;
        state.count += 1;
        Ok(())
    }
    pub(crate) fn release(&self, bytes: u64, count: usize) {
        let mut state = self.state();
        state.bytes -= bytes;
        state.count -= count;
    }
    pub(crate) fn set_limit(&self, limit: u64) {
        self.state().limit = limit;
    }
}

/// The linear color format of a GPU render target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RenderTargetFormat {
    /// Eight-bit normalized red, green, blue, and coverage alpha.
    Rgba8Unorm,
    /// Eight-bit sRGB RGBA. Rendering and sampling convert RGB automatically.
    Rgba8UnormSrgb,
    /// Eight-bit sRGB BGRA storage, with RGBA shader channels.
    Bgra8UnormSrgb,
    /// Half-precision linear red, green, blue, and coverage alpha, including HDR.
    Rgba16Float,
    /// One normalized red channel for masks and scalar fields.
    R8Unorm,
}

impl RenderTargetFormat {
    /// Number of bytes stored per texel.
    pub const fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::Rgba8Unorm | Self::Rgba8UnormSrgb | Self::Bgra8UnormSrgb => 4,
            Self::Rgba16Float => 8,
            Self::R8Unorm => 1,
        }
    }
}

/// Dimensions and format of a target owned by one window's GPU device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderTargetDescriptor {
    /// Width in physical device pixels.
    pub width: u32,
    /// Height in physical device pixels.
    pub height: u32,
    /// Linear pixel format. RGB is stored premultiplied by coverage alpha.
    pub format: RenderTargetFormat,
}

impl RenderTargetDescriptor {
    /// Construct an eight-bit RGBA target descriptor.
    pub const fn rgba8(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            format: RenderTargetFormat::Rgba8Unorm,
        }
    }

    /// Validate dimensions and calculate payload bytes without allocating.
    pub fn byte_len(self) -> Result<u64, RenderTargetError> {
        if self.width == 0
            || self.height == 0
            || self.width > MAX_TARGET_DIMENSION
            || self.height > MAX_TARGET_DIMENSION
        {
            return Err(RenderTargetError::InvalidDimensions);
        }
        u64::from(self.width)
            .checked_mul(u64::from(self.height))
            .and_then(|pixels| pixels.checked_mul(u64::from(self.format.bytes_per_pixel())))
            .ok_or(RenderTargetError::InvalidDimensions)
    }
}

#[derive(Debug)]
pub(crate) struct RenderTargetLease {
    revision: AtomicU64,
    valid: AtomicBool,
    device_valid: Arc<AtomicBool>,
}

/// A GPU texture owned by the window that created it.
///
/// Clones keep the target alive. Targets can be sampled by another custom pass
/// or displayed by [`crate::render_target`], without reading pixels back to the
/// CPU. They cannot cross windows/devices, survive device loss, or be resized;
/// create a replacement target when the desired physical size changes.
#[derive(Clone, Debug)]
pub struct RenderTarget {
    owner: u64,
    id: u64,
    descriptor: RenderTargetDescriptor,
    lease: Arc<RenderTargetLease>,
}

impl PartialEq for RenderTarget {
    fn eq(&self, other: &Self) -> bool {
        self.owner == other.owner && self.id == other.id
    }
}
impl Eq for RenderTarget {}

impl RenderTarget {
    /// Target dimensions and format.
    pub fn descriptor(&self) -> RenderTargetDescriptor {
        self.descriptor
    }

    /// Size in physical device pixels.
    pub fn size(&self) -> Size<DevicePixels> {
        size(
            DevicePixels(self.descriptor.width as i32),
            DevicePixels(self.descriptor.height as i32),
        )
    }

    /// Whether the owning renderer is still alive.
    pub fn is_valid(&self) -> bool {
        self.lease.valid.load(Ordering::Acquire) && self.lease.device_valid.load(Ordering::Acquire)
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }
    pub(crate) fn owner(&self) -> u64 {
        self.owner
    }
    pub(crate) fn revision(&self) -> u64 {
        self.lease.revision.load(Ordering::Acquire)
    }
    pub(crate) fn did_render(&self) {
        if self
            .lease
            .revision
            .fetch_update(Ordering::Release, Ordering::Relaxed, |revision| {
                revision.checked_add(1)
            })
            .is_err()
        {
            self.invalidate();
        }
    }
    pub(crate) fn invalidate(&self) {
        self.lease.valid.store(false, Ordering::Release);
    }
    #[cfg(any(
        test,
        target_os = "windows",
        all(target_os = "macos", not(feature = "macos-blade"))
    ))]
    pub(crate) fn invalidate_device(&self) {
        self.lease.device_valid.store(false, Ordering::Release);
    }
}

/// Sampling behavior for a custom pass's sampler binding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShaderSampler {
    /// Bilinear filtering with edge replication.
    #[default]
    LinearClamp,
    /// Point filtering with edge replication.
    NearestClamp,
}

/// A value supplied for one reflected resource binding in group zero.
#[derive(Clone, Debug)]
pub enum ShaderBinding {
    /// Uniform bytes laid out according to WGSL reflection, including padding.
    Uniform(Arc<[u8]>),
    /// A previously rendered target from the same window. Samples are linear,
    /// premultiplied RGBA; unassociate RGB if a shader needs straight colors.
    Texture(RenderTarget),
    /// Texture sampling behavior.
    Sampler(ShaderSampler),
}

/// Resource values keyed by a shader's WGSL `@binding` number.
#[derive(Clone, Debug, Default)]
pub struct ShaderBindings {
    pub(crate) values: BTreeMap<u32, ShaderBinding>,
}

impl ShaderBindings {
    /// Empty bindings for a procedural shader without resources.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace one binding value.
    pub fn with(mut self, binding: u32, value: ShaderBinding) -> Self {
        self.values.insert(binding, value);
        self
    }

    /// Borrow a supplied value.
    pub fn get(&self, binding: u32) -> Option<&ShaderBinding> {
        self.values.get(&binding)
    }

    pub(crate) fn validate<T>(
        &self,
        shader: &crate::ShaderHandle,
        target: &RenderTarget,
        registry: &TargetRegistry<T>,
    ) -> Result<(), RenderTargetError> {
        registry.get(target)?;
        if self.values.len() != shader.resources().len() {
            return Err(RenderTargetError::InvalidBindings(
                "resource count does not match the shader".into(),
            ));
        }
        for resource in shader.resources() {
            let value = self.get(resource.binding).ok_or_else(|| {
                RenderTargetError::InvalidBindings(format!("missing binding {}", resource.binding))
            })?;
            match (&resource.kind, value) {
                (crate::ShaderResourceKind::Uniform(layout), ShaderBinding::Uniform(bytes))
                    if bytes.len() == layout.size as usize => {}
                (crate::ShaderResourceKind::Texture2d, ShaderBinding::Texture(input)) => {
                    registry.get(input)?;
                    if input == target {
                        return Err(RenderTargetError::FeedbackLoop);
                    }
                }
                (crate::ShaderResourceKind::Sampler, ShaderBinding::Sampler(_)) => {}
                _ => {
                    return Err(RenderTargetError::InvalidBindings(format!(
                        "binding {} has the wrong type or uniform size",
                        resource.binding
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Explicit CPU readback of tightly packed, premultiplied target pixels.
///
/// sRGB formats retain their sRGB-encoded storage bytes; linear formats retain
/// linear channels. Eight-bit color channels are RGBA bytes (including BGRA targets, which are
/// reordered); R8 targets contain one red byte per texel. Half-float channels
/// are little-endian IEEE 754 binary16 values. This operation is intended for
/// exports/tests; display uses the GPU texture directly.
#[derive(Clone, Debug)]
pub struct RenderTargetReadback {
    /// Target descriptor identifying dimensions and channel format.
    pub descriptor: RenderTargetDescriptor,
    /// Tightly packed row-major pixels, with the top-left texel first.
    pub pixels: Vec<u8>,
}

/// A rejected target allocation, shader execution, display, or readback.
#[derive(Debug, thiserror::Error)]
pub enum RenderTargetError {
    /// Width/height must be nonzero and at most 16,384 device pixels.
    #[error("render target dimensions must be in 1..=16384")]
    InvalidDimensions,
    /// The request exceeds the configured per-window target byte budget.
    #[error("render target exceeds the window's GPU target budget")]
    BudgetExceeded,
    /// At most 64 live targets and 64 custom pipelines may be retained.
    #[error("render target or custom pipeline resource count limit exceeded")]
    ResourceLimit,
    /// A handle belongs to another device/window or has lost its owning device.
    #[error("render target belongs to a different or destroyed GPU device")]
    WrongDevice,
    /// A resource value does not match the shader's reflected binding layout.
    #[error("invalid shader bindings: {0}")]
    InvalidBindings(String),
    /// Rendering into a texture while sampling it is unsupported.
    #[error("a custom pass cannot sample its output target")]
    FeedbackLoop,
    /// This platform/format lacks the required GPU feature.
    #[error("custom GPU rendering is unavailable: {0}")]
    Unsupported(&'static str),
    /// GPU compilation, submission, or readback failed.
    #[error("custom GPU rendering failed: {0}")]
    Backend(String),
}

pub(crate) struct TargetResource<T> {
    pub(crate) resource: T,
    lease: Weak<RenderTargetLease>,
    bytes: u64,
}

/// Common ownership and budget accounting; backend objects remain on their
/// renderer thread and are never exposed through public handles.
pub(crate) struct TargetRegistry<T> {
    owner: u64,
    next_id: u64,
    budget: DeviceBudget,
    bytes: u64,
    resources: BTreeMap<u64, TargetResource<T>>,
}

impl<T> Default for TargetRegistry<T> {
    fn default() -> Self {
        let budget = DeviceBudget::default();
        let owner = budget.owner();
        Self {
            owner,
            next_id: 1,
            budget,
            bytes: 0,
            resources: BTreeMap::new(),
        }
    }
}

impl<T> TargetRegistry<T> {
    pub(crate) fn invalidate_device(&self) {
        self.budget.invalidate();
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn device_budget(&self) -> DeviceBudget {
        self.budget.clone()
    }
    pub(crate) fn check_request(
        &self,
        descriptor: RenderTargetDescriptor,
    ) -> Result<u64, RenderTargetError> {
        let bytes = descriptor.byte_len()?;
        self.budget.check_request(bytes)?;
        Ok(bytes)
    }

    pub(crate) fn check_allocation(
        &self,
        descriptor: RenderTargetDescriptor,
    ) -> Result<u64, RenderTargetError> {
        let bytes = self.check_request(descriptor)?;
        self.budget.check_allocation(bytes)?;
        if self.resources.len() >= MAX_RENDER_TARGETS || self.next_id == u64::MAX {
            return Err(RenderTargetError::ResourceLimit);
        }
        Ok(bytes)
    }

    pub(crate) fn insert(
        &mut self,
        descriptor: RenderTargetDescriptor,
        resource: T,
        bytes: u64,
    ) -> Result<RenderTarget, RenderTargetError> {
        self.check_allocation(descriptor)?;
        self.budget.reserve(bytes)?;
        let id = self.next_id;
        self.next_id += 1;
        let lease = Arc::new(RenderTargetLease {
            revision: AtomicU64::new(0),
            valid: AtomicBool::new(true),
            device_valid: self.budget.validity(),
        });
        self.resources.insert(
            id,
            TargetResource {
                resource,
                lease: Arc::downgrade(&lease),
                bytes,
            },
        );
        self.bytes += bytes;
        Ok(RenderTarget {
            owner: self.owner,
            id,
            descriptor,
            lease,
        })
    }

    pub(crate) fn get(&self, target: &RenderTarget) -> Result<&T, RenderTargetError> {
        if target.owner != self.owner || !target.is_valid() {
            return Err(RenderTargetError::WrongDevice);
        }
        self.resources
            .get(&target.id)
            .map(|entry| &entry.resource)
            .ok_or(RenderTargetError::WrongDevice)
    }

    pub(crate) fn take_unused(&mut self) -> Vec<T> {
        let unused: Vec<_> = self
            .resources
            .iter()
            .filter_map(|(&id, entry)| (entry.lease.strong_count() == 0).then_some(id))
            .collect();
        unused
            .into_iter()
            .map(|id| {
                let entry = self.resources.remove(&id).expect("indexed target exists");
                self.bytes -= entry.bytes;
                self.budget.release(entry.bytes, 1);
                entry.resource
            })
            .collect()
    }

    #[allow(dead_code)] // Explicit-destruction backends use this; Metal uses RAII.
    pub(crate) fn invalidate_and_drain(&mut self) -> Vec<T> {
        let resources = std::mem::take(&mut self.resources);
        self.budget.release(self.bytes, resources.len());
        self.bytes = 0;
        resources
            .into_values()
            .map(|entry| {
                if let Some(lease) = entry.lease.upgrade() {
                    lease.valid.store(false, Ordering::Release);
                }
                entry.resource
            })
            .collect()
    }

    pub(crate) fn set_budget(&mut self, bytes: u64) {
        self.budget.set_limit(bytes);
    }
    #[allow(dead_code)] // Accounting assertions in backend/device tests.
    pub(crate) fn used_bytes(&self) -> u64 {
        self.bytes
    }
}

impl<T> Drop for TargetRegistry<T> {
    fn drop(&mut self) {
        self.budget.invalidate();
        self.budget.release(self.bytes, self.resources.len());
        for entry in self.resources.values() {
            if let Some(lease) = entry.lease.upgrade() {
                lease.valid.store(false, Ordering::Release);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_registry_preserves_live_handles_and_rejects_foreign_devices() {
        let mut first = TargetRegistry::default();
        let second = TargetRegistry::<u8>::default();
        let descriptor = RenderTargetDescriptor::rgba8(4, 4);
        let target = first.insert(descriptor, 17, 64).unwrap();
        let clone = target.clone();
        assert!(matches!(
            second.get(&target),
            Err(RenderTargetError::WrongDevice)
        ));
        drop(target);
        assert!(first.take_unused().is_empty());
        assert_eq!(*first.get(&clone).unwrap(), 17);
        drop(clone);
        assert_eq!(first.take_unused(), [17]);
        assert_eq!(first.used_bytes(), 0);
    }

    #[test]
    fn impossible_allocations_preserve_existing_targets_and_device_drop_invalidates() {
        let mut registry = TargetRegistry::default();
        registry.set_budget(64);
        let descriptor = RenderTargetDescriptor::rgba8(4, 4);
        let target = registry.insert(descriptor, (), 64).unwrap();
        assert!(matches!(
            registry.check_allocation(RenderTargetDescriptor::rgba8(5, 4)),
            Err(RenderTargetError::BudgetExceeded)
        ));
        assert_eq!(registry.used_bytes(), 64);
        assert!(target.is_valid());
        drop(registry);
        assert!(!target.is_valid());
    }

    #[test]
    fn descriptors_reject_zero_dimensions_and_bounded_count_preserves_all_handles() {
        assert!(RenderTargetDescriptor::rgba8(0, 1).byte_len().is_err());
        assert!(RenderTargetDescriptor::rgba8(16_385, 1).byte_len().is_err());
        let mut registry = TargetRegistry::default();
        let descriptor = RenderTargetDescriptor::rgba8(1, 1);
        let handles: Vec<_> = (0..64)
            .map(|_| registry.insert(descriptor, (), 4).unwrap())
            .collect();
        assert!(matches!(
            registry.check_allocation(descriptor),
            Err(RenderTargetError::ResourceLimit)
        ));
        assert!(handles.iter().all(RenderTarget::is_valid));
    }
}

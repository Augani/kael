//! Private admission for the native pixels and RGBA conversion of built-in static images.
//!
//! This is not an encoded-input, ready-output, codec-private, animated-image or SVG budget.

use super::{
    MAX_DECODED_IMAGE_BYTES, checked_image_frame_len, image_decode_limits, rgba_to_bgra,
    validate_image_source_bytes,
};
use crate::Result;
use futures::channel::oneshot;
use image::{ColorType, Frame, ImageDecoder, ImageFormat, ImageReader};
use smallvec::SmallVec;
use std::{
    collections::VecDeque,
    io::Cursor,
    sync::{Arc, Mutex, MutexGuard},
};

// DynamicImage's largest currently supported native representation is RGBA32F (16 B/pixel).
// Every individually valid 256 MiB RGBA8 output therefore fits, including native + conversion.
const MAX_NATIVE_BYTES_PER_PIXEL: usize = 16;
const IMAGE_WORKING_BYTES: usize = MAX_DECODED_IMAGE_BYTES * (MAX_NATIVE_BYTES_PER_PIXEL / 4 + 1);
// Both built-in loaders hold one of the same four pipeline slots before entering this queue.
pub(crate) const IMAGE_LOAD_PIPELINES: usize = 4;
const MAX_IMAGE_WORKING_WAITERS: usize = IMAGE_LOAD_PIPELINES;

pub(crate) struct ImageWorkingBudget {
    capacity: usize,
    state: Mutex<WorkingState>,
}

struct WorkingState {
    reserved: usize,
    waiters: VecDeque<WaitingImage>,
}

struct WaitingImage {
    identity: Arc<()>,
    bytes: usize,
    sender: oneshot::Sender<ImageWorkingReservation>,
}

pub(crate) struct ImageWorkingReservation {
    budget: Arc<ImageWorkingBudget>,
    bytes: usize,
}

struct WaitingReservation {
    budget: Arc<ImageWorkingBudget>,
    identity: Arc<()>,
}

impl Default for ImageWorkingBudget {
    fn default() -> Self {
        Self::with_capacity(IMAGE_WORKING_BYTES)
    }
}

impl ImageWorkingBudget {
    fn with_capacity(capacity: usize) -> Self {
        assert!(capacity > 0 && capacity <= isize::MAX as usize);
        Self {
            capacity,
            state: Mutex::new(WorkingState {
                reserved: 0,
                waiters: VecDeque::with_capacity(MAX_IMAGE_WORKING_WAITERS),
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, WorkingState> {
        // No user code runs under this mutex. Recovery keeps cleanup usable during unwind.
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn try_acquire(self: &Arc<Self>, bytes: usize) -> Result<Option<ImageWorkingReservation>> {
        anyhow::ensure!(
            bytes > 0 && bytes <= self.capacity,
            "static image working request exceeds the App budget"
        );
        let mut state = self.state();
        if !state.waiters.is_empty() || bytes > self.capacity - state.reserved {
            return Ok(None);
        }
        state.reserved += bytes;
        Ok(Some(ImageWorkingReservation {
            budget: self.clone(),
            bytes,
        }))
    }

    pub(crate) async fn acquire(self: &Arc<Self>, bytes: usize) -> Result<ImageWorkingReservation> {
        anyhow::ensure!(
            bytes > 0 && bytes <= self.capacity,
            "static image working request exceeds the App budget"
        );
        let (sender, receiver) = oneshot::channel();
        let identity = Arc::new(());
        let waiting = WaitingReservation {
            budget: self.clone(),
            identity: identity.clone(),
        };
        {
            let mut state = self.state();
            anyhow::ensure!(
                state.waiters.len() < MAX_IMAGE_WORKING_WAITERS,
                "static image working wait queue is full"
            );
            state.waiters.push_back(WaitingImage {
                identity,
                bytes,
                sender,
            });
        }
        self.admit_waiters();
        let reservation = receiver
            .await
            .map_err(|_| anyhow::anyhow!("static image working reservation was cancelled"))?;
        // Admission removes the queue entry. Dropping a pending future instead removes it here.
        drop(waiting);
        Ok(reservation)
    }

    fn admit_waiters(self: &Arc<Self>) {
        let mut admitted = SmallVec::<
            [(
                oneshot::Sender<ImageWorkingReservation>,
                ImageWorkingReservation,
            ); MAX_IMAGE_WORKING_WAITERS],
        >::new();
        {
            let mut state = self.state();
            while let Some(first) = state.waiters.front() {
                // Do not let a new small request pass an older large request.
                if first.bytes > self.capacity - state.reserved {
                    break;
                }
                let first = state.waiters.pop_front().expect("front was present");
                state.reserved += first.bytes;
                admitted.push((
                    first.sender,
                    ImageWorkingReservation {
                        budget: self.clone(),
                        bytes: first.bytes,
                    },
                ));
            }
        }
        // Sender completion can wake arbitrary executor code. Never wake or drop a lease under
        // the metadata mutex. A cancelled receiver returns its lease and releases it below.
        for (sender, reservation) in admitted {
            let _ = sender.send(reservation);
        }
    }

    fn cancel(self: &Arc<Self>, identity: &Arc<()>) {
        let removed = {
            let mut state = self.state();
            let position = state
                .waiters
                .iter()
                .position(|waiting| Arc::ptr_eq(&waiting.identity, identity));
            position.and_then(|index| state.waiters.remove(index))
        };
        // Dropping a sender may wake its receiver, so this also happens outside the mutex.
        drop(removed);
        self.admit_waiters();
    }

    #[cfg(test)]
    pub(crate) fn for_test(capacity: usize) -> Arc<Self> {
        Arc::new(Self::with_capacity(capacity))
    }

    #[cfg(test)]
    pub(crate) fn test_snapshot(&self) -> (usize, usize) {
        let state = self.state();
        (state.reserved, state.waiters.len())
    }
}

impl Drop for WaitingReservation {
    fn drop(&mut self) {
        self.budget.cancel(&self.identity);
    }
}

impl Drop for ImageWorkingReservation {
    fn drop(&mut self) {
        {
            let mut state = self.budget.state();
            state.reserved -= self.bytes;
        }
        self.budget.admit_waiters();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StaticImagePlan {
    dimensions: (u32, u32),
    color: ColorType,
    native_bytes: usize,
    rgba_bytes: usize,
    working_bytes: usize,
}

impl StaticImagePlan {
    fn from_decoder(decoder: &(impl ImageDecoder + ?Sized), max_bytes: u64) -> Result<Self> {
        let dimensions = decoder.dimensions();
        let rgba_bytes = checked_image_frame_len(dimensions.0, dimensions.1)?;
        anyhow::ensure!(
            rgba_bytes as u64 <= max_bytes,
            "declared decoded image dimensions exceed the cache byte budget"
        );
        let color = decoder.color_type();
        let bytes_per_pixel = match color {
            ColorType::L8
            | ColorType::La8
            | ColorType::Rgb8
            | ColorType::Rgba8
            | ColorType::L16
            | ColorType::La16
            | ColorType::Rgb16
            | ColorType::Rgba16
            | ColorType::Rgb32F
            | ColorType::Rgba32F => usize::from(color.bytes_per_pixel()),
            _ => anyhow::bail!("unsupported native static image representation"),
        };
        let expected_native = (rgba_bytes / 4)
            .checked_mul(bytes_per_pixel)
            .ok_or_else(|| anyhow::anyhow!("native static image length overflowed"))?;
        let native_bytes = usize::try_from(decoder.total_bytes())
            .ok()
            .filter(|bytes| *bytes <= isize::MAX as usize)
            .ok_or_else(|| anyhow::anyhow!("native static image length cannot be represented"))?;
        anyhow::ensure!(
            native_bytes == expected_native,
            "native static image length disagrees with its representation"
        );
        let working_bytes = native_bytes
            .checked_add(rgba_bytes)
            .filter(|bytes| *bytes <= IMAGE_WORKING_BYTES)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "static image working length overflowed or exceeds the supported bound"
                )
            })?;
        Ok(Self {
            dimensions,
            color,
            native_bytes,
            rgba_bytes,
            working_bytes,
        })
    }
}

fn static_decoder(
    bytes: &[u8],
    format: ImageFormat,
    max_bytes: u64,
) -> Result<impl ImageDecoder + '_> {
    validate_image_source_bytes(bytes)?;
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image_decode_limits();
    // Preserve the existing per-decoder advisory limit; the pool is a separate strict domain.
    limits.max_alloc = Some(max_bytes.min(MAX_DECODED_IMAGE_BYTES as u64));
    reader.limits(limits);
    Ok(reader.into_decoder()?)
}

enum StaticDecodeAttempt {
    Ready(SmallVec<[Frame; 1]>),
    Waiting(StaticImagePlan),
}

fn try_decode_static_image(
    bytes: &[u8],
    format: ImageFormat,
    max_bytes: u64,
    budget: &Arc<ImageWorkingBudget>,
) -> Result<StaticDecodeAttempt> {
    let decoder = static_decoder(bytes, format, max_bytes)?;
    let plan = StaticImagePlan::from_decoder(&decoder, max_bytes)?;
    if let Some(_reservation) = budget.try_acquire(plan.working_bytes)? {
        // One header parse on the uncontended path. The lease outlives native + RGBA conversion.
        Ok(StaticDecodeAttempt::Ready(decode_planned_image(
            decoder, plan, max_bytes,
        )?))
    } else {
        // Return only scalar metadata; the opaque decoder drops before the caller can await.
        Ok(StaticDecodeAttempt::Waiting(plan))
    }
}

fn decode_planned_image(
    decoder: impl ImageDecoder,
    plan: StaticImagePlan,
    max_bytes: u64,
) -> Result<SmallVec<[Frame; 1]>> {
    anyhow::ensure!(
        StaticImagePlan::from_decoder(&decoder, max_bytes)? == plan,
        "static image header changed before pixel decode"
    );
    let image = image::DynamicImage::from_decoder(decoder)?;
    let mut rgba = image.into_rgba8();
    rgba_to_bgra(rgba.as_mut());
    Ok(SmallVec::from_elem(Frame::new(rgba), 1))
}

async fn with_static_reservation<T>(
    plan: StaticImagePlan,
    budget: &Arc<ImageWorkingBudget>,
    decode: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let _reservation = budget.acquire(plan.working_bytes).await?;
    decode()
}

pub(crate) async fn decode_static_image_with_admission(
    bytes: &[u8],
    format: ImageFormat,
    max_bytes: u64,
    budget: &Arc<ImageWorkingBudget>,
) -> Result<SmallVec<[Frame; 1]>> {
    // The opaque image decoder is not guaranteed Send. Only contention reparses its header.
    match try_decode_static_image(bytes, format, max_bytes, budget)? {
        StaticDecodeAttempt::Ready(frames) => Ok(frames),
        StaticDecodeAttempt::Waiting(plan) => {
            with_static_reservation(plan, budget, || {
                decode_planned_image(static_decoder(bytes, format, max_bytes)?, plan, max_bytes)
            })
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{
        Future,
        executor::block_on,
        task::{ArcWake, noop_waker_ref, waker_ref},
    };
    use std::{
        pin::Pin,
        sync::atomic::{AtomicUsize, Ordering},
        task::{Context, Poll},
    };

    fn poll<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
        future.poll(&mut Context::from_waker(noop_waker_ref()))
    }

    fn ready<T>(poll: Poll<Result<T>>) -> T {
        match poll {
            Poll::Ready(result) => result.unwrap(),
            Poll::Pending => panic!("expected a completed reservation"),
        }
    }

    struct ProbeDecoder {
        dimensions: (u32, u32),
        color: ColorType,
        data: Vec<u8>,
        total_bytes: Option<u64>,
        reads: Arc<AtomicUsize>,
    }

    impl ProbeDecoder {
        fn new(dimensions: (u32, u32), color: ColorType, data: Vec<u8>) -> Self {
            Self {
                dimensions,
                color,
                data,
                total_bytes: None,
                reads: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl ImageDecoder for ProbeDecoder {
        fn dimensions(&self) -> (u32, u32) {
            self.dimensions
        }
        fn color_type(&self) -> ColorType {
            self.color
        }
        fn total_bytes(&self) -> u64 {
            self.total_bytes.unwrap_or_else(|| {
                u64::from(self.dimensions.0)
                    * u64::from(self.dimensions.1)
                    * u64::from(self.color.bytes_per_pixel())
            })
        }
        fn read_image(self, output: &mut [u8]) -> image::ImageResult<()> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            output.copy_from_slice(&self.data);
            Ok(())
        }
        fn read_image_boxed(self: Box<Self>, output: &mut [u8]) -> image::ImageResult<()> {
            (*self).read_image(output)
        }
    }

    #[test]
    fn image_working_capacity_covers_all_current_dynamic_pixel_types_without_decoding() {
        assert_eq!(IMAGE_WORKING_BYTES, 1_280 * 1024 * 1024);
        for color in [
            ColorType::L8,
            ColorType::La8,
            ColorType::Rgb8,
            ColorType::Rgba8,
            ColorType::L16,
            ColorType::La16,
            ColorType::Rgb16,
            ColorType::Rgba16,
            ColorType::Rgb32F,
            ColorType::Rgba32F,
        ] {
            let decoder = ProbeDecoder::new((8192, 8192), color, Vec::new());
            let plan =
                StaticImagePlan::from_decoder(&decoder, MAX_DECODED_IMAGE_BYTES as u64).unwrap();
            assert_eq!(plan.rgba_bytes, MAX_DECODED_IMAGE_BYTES);
            assert_eq!(
                plan.native_bytes,
                (MAX_DECODED_IMAGE_BYTES / 4) * usize::from(color.bytes_per_pixel())
            );
            assert!(plan.working_bytes <= IMAGE_WORKING_BYTES);
            assert_eq!(decoder.reads.load(Ordering::SeqCst), 0);
        }
        let largest = ProbeDecoder::new((8192, 8192), ColorType::Rgba32F, Vec::new());
        assert_eq!(
            StaticImagePlan::from_decoder(&largest, MAX_DECODED_IMAGE_BYTES as u64)
                .unwrap()
                .working_bytes,
            IMAGE_WORKING_BYTES
        );
    }

    #[test]
    fn image_working_reservation_is_whole_request_and_exact_boundary() {
        let budget = ImageWorkingBudget::for_test(10);
        let first = block_on(budget.acquire(6)).unwrap();
        let mut second = Box::pin(budget.acquire(6));
        assert!(poll(second.as_mut()).is_pending());
        assert_eq!(budget.test_snapshot(), (6, 1));
        drop(first);
        let second = ready(poll(second.as_mut()));
        assert_eq!(budget.test_snapshot(), (6, 0));
        drop(second);
        let exact = block_on(budget.acquire(10)).unwrap();
        assert_eq!(budget.test_snapshot(), (10, 0));
        assert!(block_on(budget.acquire(11)).is_err());
        assert!(block_on(budget.acquire(0)).is_err());
        assert_eq!(budget.test_snapshot(), (10, 0));
        drop(exact);
        assert_eq!(budget.test_snapshot(), (0, 0));
    }

    #[test]
    fn image_working_fifo_cancellation_cannot_bypass_or_leak_a_lease() {
        let budget = ImageWorkingBudget::for_test(10);
        let held = block_on(budget.acquire(8)).unwrap();
        let mut older = Box::pin(budget.acquire(4));
        let mut smaller = Box::pin(budget.acquire(2));
        assert!(poll(older.as_mut()).is_pending());
        assert!(poll(smaller.as_mut()).is_pending());
        assert_eq!(budget.test_snapshot(), (8, 2));
        assert!(
            budget.try_acquire(2).unwrap().is_none(),
            "a small try must not pass the older waiter"
        );
        drop(older);
        let smaller = ready(poll(smaller.as_mut()));
        assert_eq!(budget.test_snapshot(), (10, 0));
        drop(smaller);
        drop(held);
        let held = block_on(budget.acquire(10)).unwrap();
        let mut first = Box::pin(budget.acquire(10));
        let mut next = Box::pin(budget.acquire(10));
        assert!(poll(first.as_mut()).is_pending());
        assert!(poll(next.as_mut()).is_pending());
        drop(held); // first owns a full lease in its receiver, but has not polled again.
        assert_eq!(budget.test_snapshot(), (10, 1));
        drop(first); // dropping an already-admitted receiver must release and advance next.
        let next = ready(poll(next.as_mut()));
        drop(next);
        assert_eq!(budget.test_snapshot(), (0, 0));
    }

    #[test]
    fn image_working_wait_queue_is_bounded_and_does_not_retain_app_owners() {
        let budget = ImageWorkingBudget::for_test(1);
        let weak = Arc::downgrade(&budget);
        let held = block_on(budget.acquire(1)).unwrap();
        let mut waiting = (0..MAX_IMAGE_WORKING_WAITERS)
            .map(|_| Box::pin(budget.acquire(1)))
            .collect::<Vec<_>>();
        for future in &mut waiting {
            assert!(poll(future.as_mut()).is_pending());
        }
        assert_eq!(budget.test_snapshot(), (1, MAX_IMAGE_WORKING_WAITERS));
        assert!(block_on(budget.acquire(1)).is_err());
        drop(waiting);
        assert_eq!(budget.test_snapshot(), (1, 0));
        drop(held);
        drop(budget);
        assert!(weak.upgrade().is_none());
    }

    struct ReentrantWake {
        budget: Arc<ImageWorkingBudget>,
        wakes: AtomicUsize,
    }
    impl ArcWake for ReentrantWake {
        fn wake_by_ref(this: &Arc<Self>) {
            assert!(
                this.budget.state.try_lock().is_ok(),
                "woke while holding pool metadata"
            );
            this.wakes.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn image_working_completion_and_cancellation_wake_outside_the_mutex() {
        let budget = ImageWorkingBudget::for_test(2);
        let held = block_on(budget.acquire(2)).unwrap();
        let wake = Arc::new(ReentrantWake {
            budget: budget.clone(),
            wakes: AtomicUsize::new(0),
        });
        let waker = waker_ref(&wake);
        let mut waiting = Box::pin(budget.acquire(2));
        assert!(
            waiting
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        drop(held);
        assert_eq!(wake.wakes.load(Ordering::SeqCst), 1);
        drop(waiting); // release a lease that was delivered before cancellation.
        assert_eq!(budget.test_snapshot(), (0, 0));
        let held = block_on(budget.acquire(1)).unwrap();
        let mut older = Box::pin(budget.acquire(2));
        let mut smaller = Box::pin(budget.acquire(1));
        assert!(poll(older.as_mut()).is_pending());
        assert!(
            smaller
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        drop(older); // cancellation admits the smaller request and wakes it without the lock.
        assert_eq!(wake.wakes.load(Ordering::SeqCst), 2);
        assert_eq!(budget.test_snapshot(), (2, 0));
        drop(smaller);
        drop(held);
        assert_eq!(budget.test_snapshot(), (0, 0));
    }

    #[test]
    fn image_working_native_and_rgba_are_reserved_before_pixel_decode() {
        for (color, data) in [
            (
                ColorType::Rgba16,
                [u16::MAX, 0, 0, u16::MAX]
                    .into_iter()
                    .flat_map(u16::to_ne_bytes)
                    .collect::<Vec<_>>(),
            ),
            (
                ColorType::Rgb32F,
                [1.0f32, 0.0, 0.0]
                    .into_iter()
                    .flat_map(f32::to_ne_bytes)
                    .collect::<Vec<_>>(),
            ),
            (
                ColorType::Rgba32F,
                [1.0f32, 0.0, 0.0, 1.0]
                    .into_iter()
                    .flat_map(f32::to_ne_bytes)
                    .collect::<Vec<_>>(),
            ),
        ] {
            let decoder = ProbeDecoder::new((1, 1), color, data.clone());
            let reads = decoder.reads.clone();
            let plan = StaticImagePlan::from_decoder(&decoder, 4).unwrap();
            let rejected = ImageWorkingBudget::for_test(plan.working_bytes - 1);
            assert!(
                block_on(with_static_reservation(plan, &rejected, || {
                    decode_planned_image(decoder, plan, 4)
                }))
                .is_err()
            );
            assert_eq!(reads.load(Ordering::SeqCst), 0);
            assert_eq!(rejected.test_snapshot(), (0, 0));
            let decoder = ProbeDecoder::new((1, 1), color, data);
            let reads = decoder.reads.clone();
            let budget = ImageWorkingBudget::for_test(plan.working_bytes);
            let frames = block_on(with_static_reservation(plan, &budget, || {
                assert_eq!(budget.test_snapshot(), (plan.working_bytes, 0));
                decode_planned_image(decoder, plan, 4)
            }))
            .unwrap();
            assert_eq!(reads.load(Ordering::SeqCst), 1);
            assert_eq!(frames[0].buffer().as_raw(), &[0, 0, 255, 255]);
            assert_eq!(budget.test_snapshot(), (0, 0));
        }
    }

    #[test]
    fn image_working_header_overflow_budget_and_changed_plan_reject_before_pixels() {
        let mut decoder = ProbeDecoder::new((1, 1), ColorType::Rgba8, vec![1, 2, 3, 4]);
        let reads = decoder.reads.clone();
        assert!(StaticImagePlan::from_decoder(&decoder, 3).is_err());
        let plan = StaticImagePlan::from_decoder(&decoder, 4).unwrap();
        decoder.total_bytes = Some(u64::MAX);
        assert!(StaticImagePlan::from_decoder(&decoder, 4).is_err());
        decoder.total_bytes = Some(3);
        assert!(StaticImagePlan::from_decoder(&decoder, 4).is_err());
        decoder.total_bytes = None;
        decoder.dimensions = (2, 1);
        assert!(decode_planned_image(decoder, plan, 8).is_err());
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        let enormous = ProbeDecoder::new((u32::MAX, u32::MAX), ColorType::Rgba32F, Vec::new());
        assert!(StaticImagePlan::from_decoder(&enormous, u64::MAX).is_err());
        assert_eq!(enormous.reads.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn image_working_errors_and_unwind_release_and_admit_the_next_request() {
        let budget = ImageWorkingBudget::for_test(8);
        let decoder = ProbeDecoder::new((1, 1), ColorType::Rgba8, vec![1, 2, 3, 4]);
        let plan = StaticImagePlan::from_decoder(&decoder, 4).unwrap();
        let failed: Result<()> = block_on(with_static_reservation(plan, &budget, || {
            anyhow::bail!("test decode error")
        }));
        assert!(failed.is_err());
        assert_eq!(budget.test_snapshot(), (0, 0));
        let mut next = Box::pin(budget.acquire(8));
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _: Result<()> = block_on(with_static_reservation(plan, &budget, || {
                assert!(poll(next.as_mut()).is_pending());
                panic!("test pixel decoder panic");
            }));
        }));
        assert!(panic.is_err());
        let next = ready(poll(next.as_mut()));
        drop(next);
        assert_eq!(budget.test_snapshot(), (0, 0));
    }

    #[test]
    fn image_working_real_png_pixels_and_public_sync_constructor_are_unchanged() {
        let source = image::RgbaImage::from_raw(1, 1, vec![17, 34, 51, 255]).unwrap();
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut encoded, ImageFormat::Png)
            .unwrap();
        let budget = ImageWorkingBudget::for_test(8);
        let future =
            decode_static_image_with_admission(encoded.get_ref(), ImageFormat::Png, 4096, &budget);
        fn require_send(_: &impl Send) {}
        require_send(&future);
        let frames = block_on(future).unwrap();
        assert_eq!(frames[0].buffer().as_raw(), &[51, 34, 17, 255]);
        let image = super::super::RenderImage::new(frames);
        assert_eq!(image.decoded_bytes(), 4);
        assert_eq!(image.as_bytes(0), Some([51, 34, 17, 255].as_slice()));
        assert_eq!(budget.test_snapshot(), (0, 0));
        let held = block_on(budget.acquire(8)).unwrap();
        let mut contended = Box::pin(decode_static_image_with_admission(
            encoded.get_ref(),
            ImageFormat::Png,
            4096,
            &budget,
        ));
        assert!(poll(contended.as_mut()).is_pending());
        assert_eq!(budget.test_snapshot(), (8, 1));
        drop(held);
        let reopened = ready(poll(contended.as_mut()));
        assert_eq!(reopened[0].buffer().as_raw(), image.as_bytes(0).unwrap());
        assert_eq!(budget.test_snapshot(), (0, 0));
        let legacy = super::super::decode_static_image_with_budget(
            encoded.get_ref(),
            ImageFormat::Png,
            4096,
        )
        .unwrap();
        assert_eq!(legacy[0].buffer().as_raw(), image.as_bytes(0).unwrap());
    }
}

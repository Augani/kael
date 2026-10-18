use crate::{
    FontFeature, FontId, GlyphId, Pixels, PlatformTextSystem, Point, SharedString, Size, point, px,
};
use collections::FxHashMap;
use parking_lot::{Mutex, RwLock, RwLockUpgradableReadGuard};
use smallvec::SmallVec;
use std::{
    borrow::Borrow,
    collections::VecDeque,
    hash::{Hash, Hasher},
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use super::LineWrapper;

/// A laid out and styled line of text
#[derive(Default, Debug)]
pub struct LineLayout {
    /// The font size for this line
    pub font_size: Pixels,
    /// The width of the line
    pub width: Pixels,
    /// The ascent of the line
    pub ascent: Pixels,
    /// The descent of the line
    pub descent: Pixels,
    /// The shaped runs that make up this line
    pub runs: Vec<ShapedRun>,
    /// The length of the line in utf-8 bytes
    pub len: usize,
}

/// A run of text that has been shaped .
#[derive(Debug, Clone)]
pub struct ShapedRun {
    /// The font id for this run
    pub font_id: FontId,
    /// The glyphs that make up this run
    pub glyphs: Vec<ShapedGlyph>,
}

/// A single glyph, ready to paint.
#[derive(Clone, Debug)]
pub struct ShapedGlyph {
    /// The ID for this glyph, as determined by the text system.
    pub id: GlyphId,

    /// The position of this glyph in its containing line. x is the native
    /// glyph origin; y is a displacement from the line baseline, positive down.
    /// Decorations keep the line baseline and do not follow glyph displacements.
    pub position: Point<Pixels>,

    /// The index of this glyph in the original text.
    pub index: usize,

    /// Whether this glyph is an emoji
    pub is_emoji: bool,
}

impl LineLayout {
    /// The index for the character at the given x coordinate
    pub fn index_for_x(&self, x: Pixels) -> Option<usize> {
        if x >= self.width {
            None
        } else {
            for run in self.runs.iter().rev() {
                for glyph in run.glyphs.iter().rev() {
                    if glyph.position.x <= x {
                        return Some(glyph.index);
                    }
                }
            }
            Some(0)
        }
    }

    /// closest_index_for_x returns the character boundary closest to the given x coordinate
    /// (e.g. to handle aligning up/down arrow keys)
    pub fn closest_index_for_x(&self, x: Pixels) -> usize {
        let mut prev_index = 0;
        let mut prev_x = px(0.);

        for run in self.runs.iter() {
            for glyph in run.glyphs.iter() {
                if glyph.position.x >= x {
                    if glyph.position.x - x < x - prev_x {
                        return glyph.index;
                    } else {
                        return prev_index;
                    }
                }
                prev_index = glyph.index;
                prev_x = glyph.position.x;
            }
        }

        if self.len == 1 {
            if x > self.width / 2. {
                return 1;
            } else {
                return 0;
            }
        }

        self.len
    }

    /// The x position of the character at the given index
    pub fn x_for_index(&self, index: usize) -> Pixels {
        for run in &self.runs {
            for glyph in &run.glyphs {
                if glyph.index >= index {
                    return glyph.position.x;
                }
            }
        }
        self.width
    }

    /// The corresponding Font at the given index
    pub fn font_id_for_index(&self, index: usize) -> Option<FontId> {
        for run in &self.runs {
            for glyph in &run.glyphs {
                if glyph.index >= index {
                    return Some(run.font_id);
                }
            }
        }
        None
    }

    fn compute_wrap_boundaries(
        &self,
        text: &str,
        wrap_width: Pixels,
        max_lines: Option<usize>,
    ) -> SmallVec<[WrapBoundary; 1]> {
        let mut boundaries = SmallVec::new();
        let mut first_non_whitespace_ix = None;
        let mut last_candidate_ix = None;
        let mut last_candidate_x = px(0.);
        let mut last_boundary = WrapBoundary {
            run_ix: 0,
            glyph_ix: 0,
        };
        let mut last_boundary_x = px(0.);
        let mut prev_ch = '\0';
        let mut glyphs = self
            .runs
            .iter()
            .enumerate()
            .flat_map(move |(run_ix, run)| {
                run.glyphs.iter().enumerate().map(move |(glyph_ix, glyph)| {
                    let character = text[glyph.index..].chars().next().unwrap();
                    (
                        WrapBoundary { run_ix, glyph_ix },
                        character,
                        glyph.position.x,
                    )
                })
            })
            .peekable();

        while let Some((boundary, ch, x)) = glyphs.next() {
            if ch == '\n' {
                continue;
            }

            // Here is very similar to `LineWrapper::wrap_line` to determine text wrapping,
            // but there are some differences, so we have to duplicate the code here.
            if LineWrapper::is_word_char(ch) {
                if prev_ch == ' ' && ch != ' ' && first_non_whitespace_ix.is_some() {
                    last_candidate_ix = Some(boundary);
                    last_candidate_x = x;
                }
            } else {
                if ch != ' ' && first_non_whitespace_ix.is_some() {
                    last_candidate_ix = Some(boundary);
                    last_candidate_x = x;
                }
            }

            if ch != ' ' && first_non_whitespace_ix.is_none() {
                first_non_whitespace_ix = Some(boundary);
            }

            let next_x = glyphs.peek().map_or(self.width, |(_, _, x)| *x);
            let width = next_x - last_boundary_x;

            if width > wrap_width && boundary > last_boundary {
                // When used line_clamp, we should limit the number of lines.
                if let Some(max_lines) = max_lines
                    && boundaries.len() >= max_lines.saturating_sub(1)
                {
                    break;
                }

                if let Some(last_candidate_ix) = last_candidate_ix.take() {
                    last_boundary = last_candidate_ix;
                    last_boundary_x = last_candidate_x;
                } else {
                    last_boundary = boundary;
                    last_boundary_x = x;
                }
                boundaries.push(last_boundary);
            }
            prev_ch = ch;
        }

        boundaries
    }

    pub(crate) fn wrap_boundaries_for_text(
        &self,
        text: &str,
        wrap_width: Pixels,
        max_lines: Option<usize>,
    ) -> SmallVec<[WrapBoundary; 1]> {
        self.compute_wrap_boundaries(text, wrap_width, max_lines)
    }
}

/// A line of text that has been wrapped to fit a given width
#[derive(Default, Debug)]
pub struct WrappedLineLayout {
    /// The line layout, pre-wrapping.
    pub unwrapped_layout: Arc<LineLayout>,

    /// The boundaries at which the line was wrapped
    pub wrap_boundaries: SmallVec<[WrapBoundary; 1]>,

    /// The width of the line, if it was wrapped
    pub wrap_width: Option<Pixels>,
}

/// A boundary at which a line was wrapped
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WrapBoundary {
    /// The index in the run just before the line was wrapped
    pub run_ix: usize,
    /// The index of the glyph just before the line was wrapped
    pub glyph_ix: usize,
}

impl WrappedLineLayout {
    /// The length of the underlying text, in utf8 bytes.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.unwrapped_layout.len
    }

    /// The width of this line, in pixels, whether or not it was wrapped.
    pub fn width(&self) -> Pixels {
        self.wrap_width
            .unwrap_or(Pixels::MAX)
            .min(self.unwrapped_layout.width)
    }

    /// The size of the whole wrapped text, for the given line_height.
    /// can span multiple lines if there are multiple wrap boundaries.
    pub fn size(&self, line_height: Pixels) -> Size<Pixels> {
        Size {
            width: self.width(),
            height: line_height * (self.wrap_boundaries.len() + 1),
        }
    }

    /// The ascent of a line in this layout
    pub fn ascent(&self) -> Pixels {
        self.unwrapped_layout.ascent
    }

    /// The descent of a line in this layout
    pub fn descent(&self) -> Pixels {
        self.unwrapped_layout.descent
    }

    /// The wrap boundaries in this layout
    pub fn wrap_boundaries(&self) -> &[WrapBoundary] {
        &self.wrap_boundaries
    }

    /// The font size of this layout
    pub fn font_size(&self) -> Pixels {
        self.unwrapped_layout.font_size
    }

    /// The runs in this layout, sans wrapping
    pub fn runs(&self) -> &[ShapedRun] {
        &self.unwrapped_layout.runs
    }

    /// The index corresponding to a given position in this layout for the given line height.
    ///
    /// See also [`Self::closest_index_for_position`].
    pub fn index_for_position(
        &self,
        position: Point<Pixels>,
        line_height: Pixels,
    ) -> Result<usize, usize> {
        self._index_for_position(position, line_height, false)
    }

    /// The closest index to a given position in this layout for the given line height.
    ///
    /// Closest means the character boundary closest to the given position.
    ///
    /// See also [`LineLayout::closest_index_for_x`].
    pub fn closest_index_for_position(
        &self,
        position: Point<Pixels>,
        line_height: Pixels,
    ) -> Result<usize, usize> {
        self._index_for_position(position, line_height, true)
    }

    fn _index_for_position(
        &self,
        mut position: Point<Pixels>,
        line_height: Pixels,
        closest: bool,
    ) -> Result<usize, usize> {
        let wrapped_line_ix = (position.y / line_height) as usize;

        let wrapped_line_start_index;
        let wrapped_line_start_x;
        if wrapped_line_ix > 0 {
            let Some(line_start_boundary) = self.wrap_boundaries.get(wrapped_line_ix - 1) else {
                return Err(0);
            };
            let run = &self.unwrapped_layout.runs[line_start_boundary.run_ix];
            let glyph = &run.glyphs[line_start_boundary.glyph_ix];
            wrapped_line_start_index = glyph.index;
            wrapped_line_start_x = glyph.position.x;
        } else {
            wrapped_line_start_index = 0;
            wrapped_line_start_x = Pixels::ZERO;
        };

        let wrapped_line_end_index;
        let wrapped_line_end_x;
        if wrapped_line_ix < self.wrap_boundaries.len() {
            let next_wrap_boundary_ix = wrapped_line_ix;
            let next_wrap_boundary = self.wrap_boundaries[next_wrap_boundary_ix];
            let run = &self.unwrapped_layout.runs[next_wrap_boundary.run_ix];
            let glyph = &run.glyphs[next_wrap_boundary.glyph_ix];
            wrapped_line_end_index = glyph.index;
            wrapped_line_end_x = glyph.position.x;
        } else {
            wrapped_line_end_index = self.unwrapped_layout.len;
            wrapped_line_end_x = self.unwrapped_layout.width;
        };

        let mut position_in_unwrapped_line = position;
        position_in_unwrapped_line.x += wrapped_line_start_x;
        if position_in_unwrapped_line.x < wrapped_line_start_x {
            Err(wrapped_line_start_index)
        } else if position_in_unwrapped_line.x >= wrapped_line_end_x {
            Err(wrapped_line_end_index)
        } else {
            if closest {
                Ok(self
                    .unwrapped_layout
                    .closest_index_for_x(position_in_unwrapped_line.x))
            } else {
                Ok(self
                    .unwrapped_layout
                    .index_for_x(position_in_unwrapped_line.x)
                    .unwrap())
            }
        }
    }

    /// Returns the pixel position for the given byte index.
    pub fn position_for_index(&self, index: usize, line_height: Pixels) -> Option<Point<Pixels>> {
        let mut line_start_ix = 0;
        let mut line_end_indices = self
            .wrap_boundaries
            .iter()
            .map(|wrap_boundary| {
                let run = &self.unwrapped_layout.runs[wrap_boundary.run_ix];
                let glyph = &run.glyphs[wrap_boundary.glyph_ix];
                glyph.index
            })
            .chain([self.len()])
            .enumerate();
        for (ix, line_end_ix) in line_end_indices {
            let line_y = ix as f32 * line_height;
            if index < line_start_ix {
                break;
            } else if index > line_end_ix {
                line_start_ix = line_end_ix;
                continue;
            } else {
                let line_start_x = self.unwrapped_layout.x_for_index(line_start_ix);
                let x = self.unwrapped_layout.x_for_index(index) - line_start_x;
                return Some(point(x, line_y));
            }
        }

        None
    }
}

// Separate pools prevent wrapped text from displacing every unwrapped layout.
// Together they retain at most 4,096 entries and 8 MiB of charged allocations.
const GLOBAL_CACHE_MAX_ENTRIES: usize = 2_048;
const GLOBAL_CACHE_MAX_BYTES: usize = 4 * 1024 * 1024;

struct GlobalCacheEntry<T> {
    value: T,
    recently_used: AtomicBool,
    bytes: usize,
}

struct ClockCache<T> {
    entries: FxHashMap<Arc<CacheKey>, GlobalCacheEntry<T>>,
    order: VecDeque<Arc<CacheKey>>,
    bytes: usize,
}

impl<T> Default for ClockCache<T> {
    fn default() -> Self {
        Self {
            entries: FxHashMap::default(),
            order: VecDeque::new(),
            bytes: 0,
        }
    }
}

impl<T> ClockCache<T> {
    fn insert(&mut self, key: Arc<CacheKey>, value: T, bytes: Option<usize>) {
        let Some(bytes) = bytes.filter(|bytes| *bytes <= GLOBAL_CACHE_MAX_BYTES) else {
            return;
        };
        if let Some(existing) = self.entries.get(key.as_ref() as &dyn AsCacheKeyRef) {
            // Another window may have completed the identical layout meanwhile.
            existing.recently_used.store(true, Ordering::Relaxed);
            return;
        }
        while self.entries.len() >= GLOBAL_CACHE_MAX_ENTRIES
            || bytes > GLOBAL_CACHE_MAX_BYTES - self.bytes
        {
            self.evict_one();
        }
        self.bytes += bytes;
        self.order.push_back(key.clone());
        self.entries.insert(
            key,
            GlobalCacheEntry {
                value,
                recently_used: AtomicBool::new(false),
                bytes,
            },
        );
    }

    fn evict_one(&mut self) {
        // A second-chance clock never sorts or allocates an eviction worklist.
        // Each key has one queue slot. At most two bounded revolutions find an
        // entry, even when concurrent hits mark every entry recently used.
        let visits = self.order.len().saturating_mul(2);
        for visit in 0..visits {
            let key = self.order.pop_front().expect("nonempty text cache");
            let entry = self.entries.get(&key).expect("text cache queue key");
            if visit < visits / 2 && entry.recently_used.swap(false, Ordering::Relaxed) {
                self.order.push_back(key);
                continue;
            }
            let entry = self.entries.remove(&key).unwrap();
            self.bytes -= entry.bytes;
            return;
        }
        unreachable!("bounded text clock must evict an entry");
    }
}

fn cache_key_bytes(key: &CacheKey) -> Option<usize> {
    let mut bytes = std::mem::size_of::<CacheKey>()
        .checked_add(2 * std::mem::size_of::<usize>())?
        .checked_add(key.text.len())?;
    if key.runs.spilled() {
        bytes = bytes.checked_add(
            key.runs
                .capacity()
                .checked_mul(std::mem::size_of::<FontRun>())?,
        )?;
    }
    if key.features.spilled() {
        bytes = bytes.checked_add(
            key.features
                .capacity()
                .checked_mul(std::mem::size_of::<FontFeature>())?,
        )?;
    }
    Some(bytes)
}

fn line_layout_bytes(layout: &LineLayout) -> Option<usize> {
    let mut bytes = std::mem::size_of::<LineLayout>()
        .checked_add(2 * std::mem::size_of::<usize>())?
        .checked_add(
            layout
                .runs
                .capacity()
                .checked_mul(std::mem::size_of::<ShapedRun>())?,
        )?;
    for run in &layout.runs {
        bytes = bytes.checked_add(
            run.glyphs
                .capacity()
                .checked_mul(std::mem::size_of::<ShapedGlyph>())?,
        )?;
    }
    Some(bytes)
}

pub(crate) struct GlobalLineLayoutCache {
    lines: RwLock<ClockCache<Arc<LineLayout>>>,
    wrapped_lines: RwLock<ClockCache<Arc<WrappedLineLayout>>>,
}

impl GlobalLineLayoutCache {
    pub fn new() -> Self {
        Self {
            lines: RwLock::new(ClockCache::default()),
            wrapped_lines: RwLock::new(ClockCache::default()),
        }
    }

    pub(crate) fn clear(&self) {
        // Replacement releases payloads and bookkeeping capacity immediately.
        // Current frames and caller-owned Arc layouts remain valid.
        *self.lines.write() = ClockCache::default();
        *self.wrapped_lines.write() = ClockCache::default();
    }

    fn get_line(&self, key: &dyn AsCacheKeyRef) -> Option<Arc<LineLayout>> {
        let lines = self.lines.read();
        let entry = lines.entries.get(key)?;
        entry.recently_used.store(true, Ordering::Relaxed);
        Some(entry.value.clone())
    }

    fn insert_line(&self, key: Arc<CacheKey>, layout: Arc<LineLayout>) {
        let bytes = cache_key_bytes(&key)
            .and_then(|key_bytes| line_layout_bytes(&layout)?.checked_add(key_bytes));
        self.lines.write().insert(key, layout, bytes);
    }

    fn get_wrapped_line(&self, key: &dyn AsCacheKeyRef) -> Option<Arc<WrappedLineLayout>> {
        let wrapped = self.wrapped_lines.read();
        let entry = wrapped.entries.get(key)?;
        entry.recently_used.store(true, Ordering::Relaxed);
        Some(entry.value.clone())
    }

    fn insert_wrapped_line(&self, key: Arc<CacheKey>, layout: Arc<WrappedLineLayout>) {
        // Charging the underlying layout in both pools is conservative even
        // when the Arc is shared; wrapped retention cannot evade the byte cap.
        let bytes = (|| {
            let bytes = cache_key_bytes(&key)?
                .checked_add(line_layout_bytes(&layout.unwrapped_layout)?)?
                .checked_add(std::mem::size_of::<WrappedLineLayout>())?
                .checked_add(2 * std::mem::size_of::<usize>())?;
            if layout.wrap_boundaries.spilled() {
                bytes.checked_add(
                    layout
                        .wrap_boundaries
                        .capacity()
                        .checked_mul(std::mem::size_of::<WrapBoundary>())?,
                )
            } else {
                Some(bytes)
            }
        })();
        self.wrapped_lines.write().insert(key, layout, bytes);
    }
}

pub(crate) struct LineLayoutCache {
    previous_frame: Mutex<FrameCache>,
    current_frame: RwLock<FrameCache>,
    platform_text_system: Arc<dyn PlatformTextSystem>,
    global_cache: Arc<GlobalLineLayoutCache>,
}

#[derive(Copy, Clone)]
struct LineLayoutOptions<'a> {
    force_width: Option<Pixels>,
    letter_spacing: Option<Pixels>,
    features: &'a [FontFeature],
}

#[derive(Default)]
struct FrameCache {
    lines: FxHashMap<Arc<CacheKey>, Arc<LineLayout>>,
    wrapped_lines: FxHashMap<Arc<CacheKey>, Arc<WrappedLineLayout>>,
    used_lines: Vec<Arc<CacheKey>>,
    used_wrapped_lines: Vec<Arc<CacheKey>>,
}

#[derive(Clone, Default)]
pub(crate) struct LineLayoutIndex {
    lines_index: usize,
    wrapped_lines_index: usize,
}

impl LineLayoutCache {
    pub fn new(
        platform_text_system: Arc<dyn PlatformTextSystem>,
        global_cache: Arc<GlobalLineLayoutCache>,
    ) -> Self {
        Self {
            previous_frame: Mutex::default(),
            current_frame: RwLock::default(),
            platform_text_system,
            global_cache,
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn clear(&self) {
        *self.previous_frame.lock() = FrameCache::default();
        *self.current_frame.write() = FrameCache::default();
        self.global_cache.clear();
    }

    pub fn layout_index(&self) -> LineLayoutIndex {
        let frame = self.current_frame.read();
        LineLayoutIndex {
            lines_index: frame.used_lines.len(),
            wrapped_lines_index: frame.used_wrapped_lines.len(),
        }
    }

    pub fn reuse_layouts(&self, range: Range<LineLayoutIndex>) {
        let mut previous_frame = &mut *self.previous_frame.lock();
        let mut current_frame = &mut *self.current_frame.write();

        for key in &previous_frame.used_lines[range.start.lines_index..range.end.lines_index] {
            if let Some((key, line)) = previous_frame.lines.remove_entry(key) {
                current_frame.lines.insert(key, line);
            }
            current_frame.used_lines.push(key.clone());
        }

        for key in &previous_frame.used_wrapped_lines
            [range.start.wrapped_lines_index..range.end.wrapped_lines_index]
        {
            if let Some((key, line)) = previous_frame.wrapped_lines.remove_entry(key) {
                current_frame.wrapped_lines.insert(key, line);
            }
            current_frame.used_wrapped_lines.push(key.clone());
        }
    }

    pub fn truncate_layouts(&self, index: LineLayoutIndex) {
        let mut current_frame = &mut *self.current_frame.write();
        current_frame.used_lines.truncate(index.lines_index);
        current_frame
            .used_wrapped_lines
            .truncate(index.wrapped_lines_index);
    }

    pub fn finish_frame(&self) {
        let mut prev_frame = self.previous_frame.lock();
        let mut curr_frame = self.current_frame.write();
        std::mem::swap(&mut *prev_frame, &mut *curr_frame);
        curr_frame.lines.clear();
        curr_frame.wrapped_lines.clear();
        curr_frame.used_lines.clear();
        curr_frame.used_wrapped_lines.clear();
    }

    pub fn layout_wrapped_line<Text>(
        &self,
        text: Text,
        font_size: Pixels,
        runs: &[FontRun],
        wrap_width: Option<Pixels>,
        max_lines: Option<usize>,
    ) -> Arc<WrappedLineLayout>
    where
        Text: AsRef<str>,
        SharedString: From<Text>,
    {
        let key = &CacheKeyRef {
            text: text.as_ref(),
            font_size,
            runs,
            wrap_width,
            max_lines,
            force_width: None,
            letter_spacing: None,
            features: &[],
        } as &dyn AsCacheKeyRef;

        let current_frame = self.current_frame.upgradable_read();
        if let Some(layout) = current_frame.wrapped_lines.get(key) {
            return layout.clone();
        }

        let previous_frame_entry = self.previous_frame.lock().wrapped_lines.remove_entry(key);
        if let Some((key, layout)) = previous_frame_entry {
            let mut current_frame = RwLockUpgradableReadGuard::upgrade(current_frame);
            current_frame
                .wrapped_lines
                .insert(key.clone(), layout.clone());
            current_frame.used_wrapped_lines.push(key);
            layout
        } else {
            // Check global cross-window cache
            if let Some(layout) = self.global_cache.get_wrapped_line(key) {
                let mut current_frame = RwLockUpgradableReadGuard::upgrade(current_frame);
                let key = Arc::new(CacheKey {
                    text: SharedString::from(text),
                    font_size,
                    runs: SmallVec::from(runs),
                    wrap_width,
                    max_lines,
                    force_width: None,
                    letter_spacing: None,
                    features: SmallVec::new(),
                });
                current_frame
                    .wrapped_lines
                    .insert(key.clone(), layout.clone());
                current_frame.used_wrapped_lines.push(key);
                return layout;
            }

            drop(current_frame);
            let text = SharedString::from(text);
            let unwrapped_layout = self.layout_line::<&SharedString>(&text, font_size, runs, None);
            let wrap_boundaries = if let Some(wrap_width) = wrap_width {
                unwrapped_layout.compute_wrap_boundaries(text.as_ref(), wrap_width, max_lines)
            } else {
                SmallVec::new()
            };
            let layout = Arc::new(WrappedLineLayout {
                unwrapped_layout,
                wrap_boundaries,
                wrap_width,
            });
            let key = Arc::new(CacheKey {
                text,
                font_size,
                runs: SmallVec::from(runs),
                wrap_width,
                max_lines,
                force_width: None,
                letter_spacing: None,
                features: SmallVec::new(),
            });

            let mut current_frame = self.current_frame.write();
            current_frame
                .wrapped_lines
                .insert(key.clone(), layout.clone());
            current_frame.used_wrapped_lines.push(key.clone());
            self.global_cache.insert_wrapped_line(key, layout.clone());

            layout
        }
    }

    pub fn layout_line<Text>(
        &self,
        text: Text,
        font_size: Pixels,
        runs: &[FontRun],
        force_width: Option<Pixels>,
    ) -> Arc<LineLayout>
    where
        Text: AsRef<str>,
        SharedString: From<Text>,
    {
        self.layout_line_with_options(
            text,
            font_size,
            runs,
            LineLayoutOptions {
                force_width,
                letter_spacing: None,
                features: &[],
            },
        )
    }

    pub fn layout_line_with_spacing<Text>(
        &self,
        text: Text,
        font_size: Pixels,
        runs: &[FontRun],
        force_width: Option<Pixels>,
        letter_spacing: Option<Pixels>,
    ) -> Arc<LineLayout>
    where
        Text: AsRef<str>,
        SharedString: From<Text>,
    {
        self.layout_line_with_options(
            text,
            font_size,
            runs,
            LineLayoutOptions {
                force_width,
                letter_spacing,
                features: &[],
            },
        )
    }

    pub fn layout_line_with_features<Text>(
        &self,
        text: Text,
        font_size: Pixels,
        runs: &[FontRun],
        force_width: Option<Pixels>,
        features: &[FontFeature],
    ) -> Arc<LineLayout>
    where
        Text: AsRef<str>,
        SharedString: From<Text>,
    {
        self.layout_line_with_options(
            text,
            font_size,
            runs,
            LineLayoutOptions {
                force_width,
                letter_spacing: None,
                features,
            },
        )
    }

    fn layout_line_with_options<Text>(
        &self,
        text: Text,
        font_size: Pixels,
        runs: &[FontRun],
        options: LineLayoutOptions<'_>,
    ) -> Arc<LineLayout>
    where
        Text: AsRef<str>,
        SharedString: From<Text>,
    {
        let key = &CacheKeyRef {
            text: text.as_ref(),
            font_size,
            runs,
            wrap_width: None,
            max_lines: None,
            force_width: options.force_width,
            letter_spacing: options.letter_spacing,
            features: options.features,
        } as &dyn AsCacheKeyRef;

        let current_frame = self.current_frame.upgradable_read();
        if let Some(layout) = current_frame.lines.get(key) {
            return layout.clone();
        }

        let mut current_frame = RwLockUpgradableReadGuard::upgrade(current_frame);
        if let Some((key, layout)) = self.previous_frame.lock().lines.remove_entry(key) {
            current_frame.lines.insert(key.clone(), layout.clone());
            current_frame.used_lines.push(key);
            return layout;
        }

        // Check global cross-window cache
        if let Some(layout) = self.global_cache.get_line(key) {
            let key = Arc::new(CacheKey {
                text: SharedString::from(text),
                font_size,
                runs: SmallVec::from(runs),
                wrap_width: None,
                max_lines: None,
                force_width: options.force_width,
                letter_spacing: options.letter_spacing,
                features: options.features.iter().cloned().collect(),
            });
            current_frame.lines.insert(key.clone(), layout.clone());
            current_frame.used_lines.push(key);
            return layout;
        }

        let text = SharedString::from(text);
        let mut layout = if options.features.is_empty() {
            self.platform_text_system
                .layout_line(&text, font_size, runs)
        } else {
            self.platform_text_system.layout_line_with_features(
                &text,
                font_size,
                runs,
                options.features,
            )
        };

        if let Some(force_width) = options.force_width {
            let mut glyph_pos = 0;
            for run in layout.runs.iter_mut() {
                for glyph in run.glyphs.iter_mut() {
                    if (glyph.position.x - glyph_pos * force_width).abs() > px(1.) {
                        glyph.position.x = glyph_pos * force_width;
                    }
                    glyph_pos += 1;
                }
            }
        }

        if let Some(spacing) = options.letter_spacing {
            let mut glyph_index: usize = 0;
            for run in layout.runs.iter_mut() {
                for glyph in run.glyphs.iter_mut() {
                    glyph.position.x = glyph.position.x + spacing * glyph_index as f32;
                    glyph_index += 1;
                }
            }
            let total_glyphs = glyph_index;
            if total_glyphs > 1 {
                layout.width = layout.width + spacing * (total_glyphs - 1) as f32;
            }
        }

        let key = Arc::new(CacheKey {
            text,
            font_size,
            runs: SmallVec::from(runs),
            wrap_width: None,
            max_lines: None,
            force_width: options.force_width,
            letter_spacing: options.letter_spacing,
            features: options.features.iter().cloned().collect(),
        });
        let layout = Arc::new(layout);
        current_frame.lines.insert(key.clone(), layout.clone());
        current_frame.used_lines.push(key.clone());
        self.global_cache.insert_line(key, layout.clone());
        layout
    }
}

/// A run of text with a single font.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct FontRun {
    pub(crate) len: usize,
    pub(crate) font_id: FontId,
}

trait AsCacheKeyRef {
    fn as_cache_key_ref(&self) -> CacheKeyRef<'_>;
}

#[derive(Clone, Debug, Eq)]
struct CacheKey {
    text: SharedString,
    font_size: Pixels,
    runs: SmallVec<[FontRun; 1]>,
    wrap_width: Option<Pixels>,
    max_lines: Option<usize>,
    force_width: Option<Pixels>,
    letter_spacing: Option<Pixels>,
    features: SmallVec<[FontFeature; 4]>,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
struct CacheKeyRef<'a> {
    text: &'a str,
    font_size: Pixels,
    runs: &'a [FontRun],
    wrap_width: Option<Pixels>,
    max_lines: Option<usize>,
    force_width: Option<Pixels>,
    letter_spacing: Option<Pixels>,
    features: &'a [FontFeature],
}

impl PartialEq for dyn AsCacheKeyRef + '_ {
    fn eq(&self, other: &dyn AsCacheKeyRef) -> bool {
        self.as_cache_key_ref() == other.as_cache_key_ref()
    }
}

impl Eq for dyn AsCacheKeyRef + '_ {}

impl Hash for dyn AsCacheKeyRef + '_ {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_cache_key_ref().hash(state)
    }
}

impl AsCacheKeyRef for CacheKey {
    fn as_cache_key_ref(&self) -> CacheKeyRef<'_> {
        CacheKeyRef {
            text: &self.text,
            font_size: self.font_size,
            runs: self.runs.as_slice(),
            wrap_width: self.wrap_width,
            max_lines: self.max_lines,
            force_width: self.force_width,
            letter_spacing: self.letter_spacing,
            features: self.features.as_slice(),
        }
    }
}

impl PartialEq for CacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.as_cache_key_ref().eq(&other.as_cache_key_ref())
    }
}

impl Hash for CacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_cache_key_ref().hash(state);
    }
}

impl<'a> Borrow<dyn AsCacheKeyRef + 'a> for Arc<CacheKey> {
    fn borrow(&self) -> &(dyn AsCacheKeyRef + 'a) {
        self.as_ref() as &dyn AsCacheKeyRef
    }
}

impl AsCacheKeyRef for CacheKeyRef<'_> {
    fn as_cache_key_ref(&self) -> CacheKeyRef<'_> {
        *self
    }
}

#[cfg(test)]
mod global_layout_cache_tests {
    use super::*;

    fn key(text: impl Into<SharedString>) -> Arc<CacheKey> {
        Arc::new(CacheKey {
            text: text.into(),
            font_size: px(14.0),
            runs: SmallVec::new(),
            wrap_width: None,
            max_lines: None,
            force_width: None,
            letter_spacing: None,
            features: SmallVec::new(),
        })
    }

    #[test]
    fn text_clock_bounds_metadata_preserves_recent_hits_and_has_one_slot_per_key() {
        let mut cache = ClockCache::default();
        let keys: Vec<_> = (0..GLOBAL_CACHE_MAX_ENTRIES)
            .map(|index| key(format!("document_{index}")))
            .collect();
        for (index, key) in keys.iter().enumerate() {
            cache.insert(key.clone(), index, Some(1));
        }
        cache.entries[&keys[0]]
            .recently_used
            .store(true, Ordering::Relaxed);
        cache.insert(key("new document"), usize::MAX, Some(1));
        assert!(cache.entries.contains_key(&keys[0]));
        assert!(!cache.entries.contains_key(&keys[1]));
        cache.insert(keys[0].clone(), usize::MAX, Some(1));
        assert_eq!(cache.entries[&keys[0]].value, 0);
        for index in 0..GLOBAL_CACHE_MAX_ENTRIES * 3 {
            cache.insert(key(format!("revision_{index}")), index, Some(1));
        }
        assert_eq!(cache.entries.len(), GLOBAL_CACHE_MAX_ENTRIES);
        assert_eq!(cache.order.len(), GLOBAL_CACHE_MAX_ENTRIES);
        let unique: std::collections::HashSet<_> = cache.order.iter().collect();
        assert_eq!(unique.len(), GLOBAL_CACHE_MAX_ENTRIES);
        assert!(
            cache
                .order
                .iter()
                .all(|key| cache.entries.contains_key(key))
        );
        assert_eq!(cache.bytes, GLOBAL_CACHE_MAX_ENTRIES);
    }

    #[test]
    fn oversized_or_overflowing_text_admission_does_not_evict_ready_layouts() {
        let mut cache = ClockCache::default();
        let retained = key("retained");
        cache.insert(retained.clone(), 1, Some(GLOBAL_CACHE_MAX_BYTES));
        cache.insert(key("oversized"), 2, Some(GLOBAL_CACHE_MAX_BYTES + 1));
        cache.insert(key("overflow"), 3, None);
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.entries[&retained].value, 1);
        assert_eq!(cache.bytes, GLOBAL_CACHE_MAX_BYTES);
        cache.insert(key("admitted"), 4, Some(1));
        assert!(!cache.entries.contains_key(&retained));
        assert_eq!(cache.bytes, 1);
    }

    #[test]
    fn text_cache_charges_allocated_glyph_capacity_and_clear_preserves_caller_handles() {
        let cache = GlobalLineLayoutCache::new();
        let retained = Arc::new(LineLayout::default());
        let weak = Arc::downgrade(&retained);
        let retained_key = key("visible 日本語");
        cache.insert_line(retained_key.clone(), retained.clone());
        let oversized = Arc::new(LineLayout {
            runs: vec![ShapedRun {
                font_id: FontId(0),
                glyphs: Vec::with_capacity(
                    GLOBAL_CACHE_MAX_BYTES / std::mem::size_of::<ShapedGlyph>(),
                ),
            }],
            ..Default::default()
        });
        cache.insert_line(key("empty but allocated glyph vector"), oversized);
        assert_eq!(cache.lines.read().entries.len(), 1);
        assert!(Arc::ptr_eq(
            &retained,
            &cache.get_line(retained_key.as_ref()).unwrap()
        ));
        cache.clear();
        assert!(cache.lines.read().entries.is_empty());
        assert_eq!(cache.lines.read().entries.capacity(), 0);
        assert_eq!(cache.lines.read().order.capacity(), 0);
        assert!(weak.upgrade().is_some());
        drop(retained);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn wrapped_cache_charges_shared_unwrapped_payload_and_releases_both_pools() {
        let cache = GlobalLineLayoutCache::new();
        let layout = Arc::new(LineLayout::default());
        let wrapped = Arc::new(WrappedLineLayout {
            unwrapped_layout: layout.clone(),
            wrap_boundaries: SmallVec::new(),
            wrap_width: Some(px(80.0)),
        });
        let weak = Arc::downgrade(&layout);
        let key = key("wrapped");
        cache.insert_line(key.clone(), layout);
        cache.insert_wrapped_line(key, wrapped.clone());
        assert!(cache.wrapped_lines.read().bytes > cache.lines.read().bytes);
        drop(wrapped);
        cache.clear();
        assert!(weak.upgrade().is_none());
        assert_eq!(cache.wrapped_lines.read().entries.capacity(), 0);
        assert_eq!(cache.wrapped_lines.read().order.capacity(), 0);
    }

    #[test]
    fn wrap_line_limit_is_part_of_frame_and_cross_window_cache_identity() {
        let global = Arc::new(GlobalLineLayoutCache::new());
        let system = Arc::new(crate::NoopTextSystem);
        let first = LineLayoutCache::new(system.clone(), global.clone());
        let text = "one two three four five six seven eight nine ten eleven twelve";
        let full = first.layout_wrapped_line(text, px(14.0), &[], Some(px(30.0)), None);
        let one = first.layout_wrapped_line(text, px(14.0), &[], Some(px(30.0)), Some(1));
        let three = first.layout_wrapped_line(text, px(14.0), &[], Some(px(30.0)), Some(3));
        let zero = first.layout_wrapped_line(text, px(14.0), &[], Some(px(30.0)), Some(0));
        assert!(full.wrap_boundaries.len() > 3);
        assert!(one.wrap_boundaries.is_empty());
        assert_eq!(three.wrap_boundaries.len(), 2);
        assert!(zero.wrap_boundaries.is_empty());
        first.finish_frame();
        let second = LineLayoutCache::new(system, global);
        for (limit, expected) in [(None, full), (Some(1), one), (Some(3), three)] {
            let actual = second.layout_wrapped_line(text, px(14.0), &[], Some(px(30.0)), limit);
            assert!(Arc::ptr_eq(&actual, &expected));
        }
    }
}

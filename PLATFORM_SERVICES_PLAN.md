# Kael Platform Services — Full Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a complete platform services layer (18 crates) that gives Kael feature parity with native macOS/SwiftUI development while providing Linux and Windows support — eliminating the need for developers to ever touch Swift/AppKit.

**Architecture:** Each service is an independent crate under `crates/kael_<name>/` with a unified pattern: public Rust API in `lib.rs`, platform backends in `platform/{mac,linux,windows}/`, re-exported via feature flags on the main `kael` crate. All APIs are async-first using the existing smol runtime.

**Tech Stack:** Rust, objc2 (macOS FFI), windows crate (Windows FFI), D-Bus (Linux), SQLite (rusqlite), rodio/cpal (audio), mupdf (PDF), ONNX Runtime (ML), btleplug (BLE)

---

## Build Order

```
Layer 0 (Foundation):     kael_storage, kael_icons, kael_diagnostics
Layer 1 (Content):        kael_audio, kael_pdf, kael_document
Layer 2 (System):         kael_notifications, kael_share, kael_search, kael_automation
Layer 3 (Device):         kael_location, kael_bluetooth, kael_maps
Layer 4 (Intelligence):   kael_ml, kael_nlp
Layer 5 (Distribution):   kael_licensing, kael_updater, kael_sync
```

---

## Layer 0: Foundation

---

### Crate 1: `kael_storage` — Data Persistence

**Purpose:** Typed key-value store + embedded SQLite database with migrations.

**Dependencies:** `rusqlite`, `serde`, `serde_json`

**Files:**
- Create: `crates/kael_storage/Cargo.toml`
- Create: `crates/kael_storage/src/lib.rs`
- Create: `crates/kael_storage/src/kv.rs`
- Create: `crates/kael_storage/src/database.rs`
- Create: `crates/kael_storage/src/migration.rs`
- Create: `crates/kael_storage/src/platform/mod.rs`
- Create: `crates/kael_storage/src/platform/mac.rs`
- Create: `crates/kael_storage/src/platform/linux.rs`
- Create: `crates/kael_storage/src/platform/windows.rs`

#### Public API

```rust
// kael_storage/src/lib.rs
pub mod kv;
pub mod database;
pub mod migration;

// Key-Value Store
pub trait KvStore: Send + Sync {
    fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T>;
    fn set<T: Serialize>(&self, key: &str, value: &T);
    fn remove(&self, key: &str);
    fn keys(&self) -> Vec<String>;
    fn observe<T: DeserializeOwned + 'static>(
        &self,
        key: &str,
        callback: impl Fn(Option<T>) + Send + 'static,
    ) -> Subscription;
}

// Database
pub struct Database { /* ... */ }

impl Database {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self>;
    pub async fn open_in_memory() -> Result<Self>;
    pub async fn migrate(&self, migrations: &[Migration]) -> Result<()>;
    pub async fn execute(&self, sql: &str, params: &[&dyn ToSql]) -> Result<usize>;
    pub async fn query<T: FromRow>(&self, sql: &str, params: &[&dyn ToSql]) -> Result<Vec<T>>;
    pub async fn query_one<T: FromRow>(&self, sql: &str, params: &[&dyn ToSql]) -> Result<T>;
    pub async fn transaction<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&Transaction) -> Result<R> + Send + 'static,
        R: Send + 'static;
}

// Migration
pub struct Migration {
    pub version: u32,
    pub description: &'static str,
    pub up: &'static str,
    pub down: Option<&'static str>,
}
```

#### Platform Backends (KV only — DB is pure Rust/SQLite)

| Platform | KV Implementation |
|----------|------------------|
| macOS | `NSUserDefaults` via objc2 for system integration, SQLite for large values |
| Linux | JSON file in `$XDG_CONFIG_HOME/<app>/preferences.json` |
| Windows | Registry (`HKCU\Software\<app>`) for small values, SQLite for large |

#### Tasks

- [ ] Create crate scaffold with Cargo.toml, workspace registration
- [ ] Implement `Migration` struct and migration runner (up/down, version tracking)
- [ ] Implement `Database` with connection pooling (r2d2 or manual pool)
- [ ] Implement `FromRow` derive macro or manual trait for row mapping
- [ ] Implement `KvStore` trait and macOS backend (`NSUserDefaults`)
- [ ] Implement Linux KV backend (JSON file with fsync)
- [ ] Implement Windows KV backend (Registry)
- [ ] Add observable KV (notify on change)
- [ ] Write tests: migration ordering, rollback, concurrent access
- [ ] Integration test: round-trip serialize/deserialize complex types
- [ ] Commit

---

### Crate 2: `kael_icons` — System Icon Library

**Purpose:** 2000+ weight-variant icons bundled with the framework, accessible via a simple API.

**Dependencies:** None external — uses existing `resvg` in kael for rendering.

**Files:**
- Create: `crates/kael_icons/Cargo.toml`
- Create: `crates/kael_icons/src/lib.rs`
- Create: `crates/kael_icons/src/catalog.rs`
- Create: `crates/kael_icons/src/weight.rs`
- Create: `crates/kael_icons/assets/` (SVG source files, organized by category)
- Create: `crates/kael_icons/build.rs` (generates Rust enum from SVG directory)

#### Public API

```rust
pub enum IconWeight {
    Thin,
    Light,
    Regular,
    Medium,
    SemiBold,
    Bold,
    Black,
}

pub enum IconName {
    Document,
    DocumentFill,
    Folder,
    FolderFill,
    Trash,
    TrashFill,
    Share,
    // ... 2000+ variants generated from asset directory
}

pub struct Icon {
    pub name: IconName,
    pub weight: IconWeight,
    pub size: Pixels,
    pub color: Option<Hsla>,
}

impl Icon {
    pub fn new(name: IconName) -> Self;
    pub fn weight(self, weight: IconWeight) -> Self;
    pub fn size(self, size: impl Into<Pixels>) -> Self;
    pub fn color(self, color: impl Into<Hsla>) -> Self;
}

impl IntoElement for Icon { /* renders via kael's SVG pipeline */ }

// Convenience macro
// icon!(folder_fill, weight: Bold, size: 24.)
```

#### Icon Source

Use **Phosphor Icons** (MIT licensed, 7000+ icons, 6 weights) as the base set. Package SVGs at build time into a compressed binary blob included via `include_bytes!`. The build script generates the `IconName` enum from the directory structure.

On macOS, optionally use SF Symbols when available via `NSImage(systemSymbolName:)` for platform-native appearance. Fall back to bundled set when SF Symbols unavailable.

#### Tasks

- [ ] Create crate scaffold
- [ ] Download and organize Phosphor Icons SVG set (select ~2000 most useful)
- [ ] Write build.rs that scans `assets/` and generates `IconName` enum
- [ ] Implement `Icon` struct with builder pattern
- [ ] Implement `IntoElement` using kael's existing SVG rendering path
- [ ] Add weight matching: auto-select weight based on surrounding text weight
- [ ] Add macOS SF Symbols bridge (optional, feature-gated)
- [ ] Write visual test example showing icon grid
- [ ] Commit

---

### Crate 3: `kael_diagnostics` — Crash Reporting & Metrics

**Purpose:** Capture crashes with full stack traces, record performance metrics, send to configurable backend.

**Dependencies:** `backtrace`, `minidump-writer` (Linux/Windows), `serde`, `uuid`

**Files:**
- Create: `crates/kael_diagnostics/Cargo.toml`
- Create: `crates/kael_diagnostics/src/lib.rs`
- Create: `crates/kael_diagnostics/src/crash.rs`
- Create: `crates/kael_diagnostics/src/breadcrumb.rs`
- Create: `crates/kael_diagnostics/src/metrics.rs`
- Create: `crates/kael_diagnostics/src/reporter.rs`
- Create: `crates/kael_diagnostics/src/platform/mac.rs`
- Create: `crates/kael_diagnostics/src/platform/linux.rs`
- Create: `crates/kael_diagnostics/src/platform/windows.rs`

#### Public API

```rust
pub struct DiagnosticsConfig {
    pub dsn: Option<String>,           // Sentry-compatible endpoint
    pub release: String,
    pub environment: String,
    pub max_breadcrumbs: usize,        // default 100
    pub sample_rate: f64,              // 0.0 - 1.0
    pub before_send: Option<Box<dyn Fn(&mut CrashReport) -> bool + Send + Sync>>,
}

pub fn init(config: DiagnosticsConfig);

pub fn add_breadcrumb(breadcrumb: Breadcrumb);

pub struct Breadcrumb {
    pub category: String,
    pub message: String,
    pub level: Level,
    pub timestamp: SystemTime,
    pub data: HashMap<String, String>,
}

pub fn capture_error(error: &dyn std::error::Error);

// Performance
pub fn start_transaction(name: &str) -> Transaction;
impl Transaction {
    pub fn start_span(&self, operation: &str) -> Span;
    pub fn finish(self);
}
impl Span {
    pub fn finish(self);
}

// Metrics
pub fn record_gauge(name: &str, value: f64);
pub fn record_counter(name: &str, delta: i64);
pub fn record_histogram(name: &str, value: f64);
```

#### Platform Crash Capture

| Platform | Mechanism |
|----------|-----------|
| macOS | Mach exception handler (`EXC_CRASH`, `EXC_BAD_ACCESS`) + `NSSetUncaughtExceptionHandler` |
| Linux | Signal handler (`SIGSEGV`, `SIGBUS`, `SIGABRT`) + minidump |
| Windows | `SetUnhandledExceptionFilter` + minidump via `MiniDumpWriteDump` |

#### Tasks

- [ ] Create crate scaffold
- [ ] Implement breadcrumb ring buffer (lock-free for signal safety)
- [ ] Implement crash handler for macOS (Mach exceptions)
- [ ] Implement crash handler for Linux (signal handler + minidump)
- [ ] Implement crash handler for Windows (SEH)
- [ ] Implement stack trace symbolication (inline, using `backtrace` crate)
- [ ] Implement `CrashReport` serialization (Sentry envelope format)
- [ ] Implement HTTP reporter (async upload on next launch)
- [ ] Implement metrics collection (gauge, counter, histogram)
- [ ] Implement performance transaction/span tracking
- [ ] Write tests: breadcrumb overflow, report serialization
- [ ] Commit

---

## Layer 1: Content

---

### Crate 4: `kael_audio` — Audio Playback & Recording

**Purpose:** Full audio playback (files, streams, spatial), recording, and audio session management.

**Dependencies:** `rodio`, `cpal`, `symphonia` (decoding), platform-specific for advanced features

**Files:**
- Create: `crates/kael_audio/Cargo.toml`
- Create: `crates/kael_audio/src/lib.rs`
- Create: `crates/kael_audio/src/player.rs`
- Create: `crates/kael_audio/src/playlist.rs`
- Create: `crates/kael_audio/src/session.rs`
- Create: `crates/kael_audio/src/spatial.rs`
- Create: `crates/kael_audio/src/effects.rs`
- Create: `crates/kael_audio/src/platform/mac.rs`
- Create: `crates/kael_audio/src/platform/linux.rs`
- Create: `crates/kael_audio/src/platform/windows.rs`

#### Public API

```rust
pub struct AudioPlayer { /* ... */ }

impl AudioPlayer {
    pub fn new(cx: &App) -> Self;
    pub async fn load(&self, source: AudioSource) -> Result<Track>;
    pub fn play(&self, track: &Track);
    pub fn pause(&self);
    pub fn stop(&self);
    pub fn seek(&self, position: Duration);
    pub fn set_volume(&self, volume: f32); // 0.0 - 1.0
    pub fn set_rate(&self, rate: f32);     // 0.5 - 2.0
    pub fn position(&self) -> Duration;
    pub fn duration(&self) -> Option<Duration>;
    pub fn state(&self) -> PlaybackState;
    pub fn on_state_change(&self, callback: impl Fn(PlaybackState) + Send + 'static) -> Subscription;
    pub fn on_position_change(&self, callback: impl Fn(Duration) + Send + 'static) -> Subscription;
}

pub enum AudioSource {
    File(PathBuf),
    Url(String),
    Memory(Arc<[u8]>),
}

pub enum PlaybackState {
    Idle,
    Loading,
    Playing,
    Paused,
    Stopped,
    Error(String),
}

pub struct Playlist {
    pub fn new() -> Self;
    pub fn add(&mut self, source: AudioSource);
    pub fn remove(&mut self, index: usize);
    pub fn next(&self);
    pub fn previous(&self);
    pub fn set_repeat(&mut self, mode: RepeatMode);
    pub fn set_shuffle(&mut self, enabled: bool);
}

pub enum RepeatMode { Off, One, All }

// Spatial Audio
pub struct SpatialAudioPlayer { /* ... */ }
impl SpatialAudioPlayer {
    pub fn set_listener_position(&self, position: [f32; 3]);
    pub fn set_listener_orientation(&self, forward: [f32; 3], up: [f32; 3]);
    pub fn set_source_position(&self, track: &Track, position: [f32; 3]);
}

// Audio Session (system integration)
pub struct AudioSession { /* ... */ }
impl AudioSession {
    pub fn set_category(&self, category: AudioCategory);
    pub fn set_active(&self, active: bool) -> Result<()>;
    pub fn current_route(&self) -> AudioRoute;
    pub fn on_route_change(&self, callback: impl Fn(AudioRoute) + Send + 'static) -> Subscription;
    pub fn on_interruption(&self, callback: impl Fn(Interruption) + Send + 'static) -> Subscription;
}

pub enum AudioCategory { Playback, Record, PlayAndRecord, Ambient }
```

#### Platform Implementation

| Platform | Playback Engine | Spatial | Session |
|----------|----------------|---------|---------|
| macOS | AVAudioEngine (Obj-C FFI) for system integration + rodio fallback | AVAudioEnvironmentNode | AVAudioSession |
| Linux | rodio + cpal (PipeWire/PulseAudio) | OpenAL Soft via C FFI | PulseAudio session |
| Windows | rodio + cpal (WASAPI) | Windows Spatial Audio API | AudioSessionControl |

For decoding: use `symphonia` (pure Rust) for MP3, FLAC, WAV, OGG, AAC. On macOS, optionally use Core Audio decoders for Apple Lossless and other proprietary formats.

#### Tasks

- [ ] Create crate scaffold
- [ ] Implement `AudioPlayer` core with rodio backend (cross-platform baseline)
- [ ] Implement `AudioSource` loading (file, memory, async URL download)
- [ ] Implement playback controls (play, pause, stop, seek, volume, rate)
- [ ] Implement position/duration tracking with callbacks
- [ ] Implement `Playlist` with repeat/shuffle
- [ ] Implement macOS AVAudioEngine backend for native integration
- [ ] Implement `AudioSession` for macOS (category, routing, interruption)
- [ ] Implement Linux audio session (PipeWire/PulseAudio route detection)
- [ ] Implement Windows audio session (WASAPI endpoint changes)
- [ ] Implement spatial audio (OpenAL Soft as cross-platform baseline)
- [ ] Implement macOS spatial audio via AVAudioEnvironmentNode
- [ ] Write tests: playback state machine, seek accuracy, playlist ordering
- [ ] Write example: simple music player with playlist
- [ ] Commit

---

### Crate 5: `kael_pdf` — PDF Rendering & Annotation

**Purpose:** Render PDF pages to GPU textures, extract text, search, annotate, fill forms.

**Dependencies:** `mupdf-sys` (C bindings to MuPDF), or pure Rust `pdf` crate for basic, mupdf for full

**Files:**
- Create: `crates/kael_pdf/Cargo.toml`
- Create: `crates/kael_pdf/src/lib.rs`
- Create: `crates/kael_pdf/src/document.rs`
- Create: `crates/kael_pdf/src/page.rs`
- Create: `crates/kael_pdf/src/renderer.rs`
- Create: `crates/kael_pdf/src/text.rs`
- Create: `crates/kael_pdf/src/annotation.rs`
- Create: `crates/kael_pdf/src/element.rs` (kael Element integration)
- Create: `crates/kael_pdf/src/platform/mac.rs` (optional PDFKit fast path)
- Create: `crates/kael_pdf/mupdf-sys/` (C binding crate)

#### Public API

```rust
pub struct PdfDocument { /* ... */ }

impl PdfDocument {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self>;
    pub async fn open_from_memory(data: &[u8]) -> Result<Self>;
    pub fn page_count(&self) -> usize;
    pub fn page(&self, index: usize) -> Result<PdfPage>;
    pub fn metadata(&self) -> PdfMetadata;
    pub fn outline(&self) -> Vec<OutlineItem>;
    pub async fn save(&self, path: impl AsRef<Path>) -> Result<()>;
}

pub struct PdfPage { /* ... */ }

impl PdfPage {
    pub fn size(&self) -> Size<Pixels>; // in points
    pub async fn render(&self, scale: f32) -> Result<RenderImage>;
    pub fn text(&self) -> String;
    pub fn search(&self, query: &str) -> Vec<TextMatch>;
    pub fn links(&self) -> Vec<PdfLink>;
    pub fn annotations(&self) -> Vec<Annotation>;
    pub fn add_annotation(&self, annotation: Annotation) -> Result<()>;
    pub fn remove_annotation(&self, id: AnnotationId) -> Result<()>;
}

pub enum Annotation {
    Highlight { rects: Vec<Bounds<Pixels>>, color: Hsla },
    Note { position: Point<Pixels>, text: String },
    FreeText { bounds: Bounds<Pixels>, text: String, font_size: f32 },
    Ink { paths: Vec<Vec<Point<Pixels>>>, color: Hsla, width: f32 },
    Stamp { bounds: Bounds<Pixels>, kind: StampKind },
}

// Kael Element for embedding in UI
pub struct PdfView { /* ... */ }

impl PdfView {
    pub fn new(document: PdfDocument) -> Self;
    pub fn current_page(&self) -> usize;
    pub fn go_to_page(&mut self, page: usize);
    pub fn zoom(&self) -> f32;
    pub fn set_zoom(&mut self, zoom: f32);
    pub fn set_display_mode(&mut self, mode: DisplayMode);
}

pub enum DisplayMode { SinglePage, Continuous, TwoUp }

impl IntoElement for PdfView { /* ... */ }
```

#### Implementation

Use **MuPDF** (AGPL or commercial license) via C FFI as the primary engine. MuPDF handles:
- PDF parsing and page tree navigation
- Page rasterization to RGBA bitmaps (which we upload as GPU textures)
- Text extraction with position information
- Annotation read/write
- Form filling
- Digital signatures

On macOS, optionally use PDFKit (`PDFDocument`, `PDFPage`) for better system integration and smaller binary size. Feature-gated: `features = ["pdfkit"]`.

#### Build System

MuPDF is ~4MB compiled. Include as a git submodule or use a `-sys` crate that downloads and compiles it. The build script compiles mupdf with:
- No JavaScript (smaller binary)
- No X11 (we handle rendering ourselves)
- Threading support enabled

#### Tasks

- [ ] Create crate scaffold and mupdf-sys binding crate
- [ ] Write build.rs for mupdf compilation (download + cc::Build)
- [ ] Implement `PdfDocument` (open, save, metadata, outline)
- [ ] Implement `PdfPage` (render to RGBA bitmap at given scale)
- [ ] Integrate page rendering with kael's texture/image system
- [ ] Implement text extraction with bounding boxes
- [ ] Implement search within page (highlight matches)
- [ ] Implement annotation read/write (highlight, note, ink, freetext)
- [ ] Implement `PdfView` kael Element (scrollable, zoomable, paginated)
- [ ] Implement keyboard navigation (page up/down, home/end)
- [ ] Add macOS PDFKit backend (feature-gated alternative)
- [ ] Implement print integration (kael already has print module)
- [ ] Write tests: open/render/search, annotation round-trip
- [ ] Write example: PDF viewer with annotations
- [ ] Commit

---

### Crate 6: `kael_document` — Document-Based App Model

**Purpose:** Document lifecycle management — new/open/save/autosave/versioning/file associations.

**Dependencies:** `kael_storage` (for version storage), `serde`

**Files:**
- Create: `crates/kael_document/Cargo.toml`
- Create: `crates/kael_document/src/lib.rs`
- Create: `crates/kael_document/src/document.rs`
- Create: `crates/kael_document/src/autosave.rs`
- Create: `crates/kael_document/src/versions.rs`
- Create: `crates/kael_document/src/recent.rs`
- Create: `crates/kael_document/src/file_type.rs`
- Create: `crates/kael_document/src/platform/mac.rs`
- Create: `crates/kael_document/src/platform/linux.rs`
- Create: `crates/kael_document/src/platform/windows.rs`

#### Public API

```rust
pub trait Document: Send + Sync + 'static {
    type Content: Serialize + DeserializeOwned + Clone + PartialEq;

    fn file_types() -> &'static [FileType];
    fn new_untitled() -> Self::Content;
    fn read(data: &[u8], file_type: &FileType) -> Result<Self::Content>;
    fn write(content: &Self::Content, file_type: &FileType) -> Result<Vec<u8>>;
}

pub struct DocumentController<D: Document> {
    /* ... */
}

impl<D: Document> DocumentController<D> {
    pub fn new(cx: &App) -> Self;
    pub fn new_document(&self) -> DocumentHandle<D>;
    pub async fn open(&self, path: impl AsRef<Path>) -> Result<DocumentHandle<D>>;
    pub fn recent_documents(&self) -> Vec<RecentDocument>;
    pub fn clear_recent(&self);
}

pub struct DocumentHandle<D: Document> {
    /* ... */
}

impl<D: Document> DocumentHandle<D> {
    pub fn content(&self) -> &D::Content;
    pub fn modify(&self, f: impl FnOnce(&mut D::Content));
    pub fn is_dirty(&self) -> bool;
    pub fn file_path(&self) -> Option<&Path>;
    pub async fn save(&self) -> Result<()>;
    pub async fn save_as(&self, path: impl AsRef<Path>) -> Result<()>;
    pub async fn revert(&self) -> Result<()>;
    pub fn undo(&self);
    pub fn redo(&self);
    pub fn can_undo(&self) -> bool;
    pub fn can_redo(&self) -> bool;

    // Version history
    pub fn versions(&self) -> Vec<DocumentVersion>;
    pub async fn restore_version(&self, version: &DocumentVersion) -> Result<()>;

    // Observe changes
    pub fn on_change(&self, callback: impl Fn(&D::Content) + Send + 'static) -> Subscription;
    pub fn on_dirty_change(&self, callback: impl Fn(bool) + Send + 'static) -> Subscription;
}

pub struct FileType {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub uti: Option<&'static str>,       // macOS UTI
    pub mime: Option<&'static str>,      // MIME type
}

pub struct AutosaveConfig {
    pub interval: Duration,              // default 30s
    pub location: AutosaveLocation,
}

pub enum AutosaveLocation {
    AdjacentToFile,                      // .filename.autosave
    SystemTemp,                          // OS temp directory
    Custom(PathBuf),
}
```

#### Platform Integration

| Platform | Recent Files | File Associations | Autosave Location |
|----------|-------------|-------------------|-------------------|
| macOS | `NSDocumentController.recentDocumentURLs` | Info.plist `CFBundleDocumentTypes` | `~/Library/Autosave Information/` |
| Linux | `gtk_recent_manager` / `recently-used.xbel` | `.desktop` file MIME types | `$XDG_DATA_HOME/<app>/autosave/` |
| Windows | Jump List via `ICustomDestinationList` | Registry `HKCR\.<ext>` | `%LOCALAPPDATA%\<app>\autosave\` |

#### Tasks

- [ ] Create crate scaffold
- [ ] Implement `Document` trait and `DocumentController`
- [ ] Implement `DocumentHandle` with dirty tracking and change notification
- [ ] Implement undo/redo via command pattern (snapshot or operation-based)
- [ ] Implement autosave (periodic timer, save on focus loss)
- [ ] Implement version history (content-addressed storage, configurable max versions)
- [ ] Implement recent documents (macOS NSDocumentController bridge)
- [ ] Implement recent documents (Linux recently-used.xbel)
- [ ] Implement recent documents (Windows Jump List)
- [ ] Implement file type registration helpers (generate Info.plist entries, .desktop files)
- [ ] Write tests: dirty tracking, autosave timing, version restore
- [ ] Write example: simple text editor with autosave and undo
- [ ] Commit

---

## Layer 2: System Integration

---

### Crate 7: `kael_notifications` — Push & Local Notifications (Extended)

**Purpose:** Extend existing local notifications with push support, actions, categories, and rich content.

**Dependencies:** Platform-specific push services, `serde`

**Files:**
- Create: `crates/kael_notifications/Cargo.toml`
- Create: `crates/kael_notifications/src/lib.rs`
- Create: `crates/kael_notifications/src/local.rs`
- Create: `crates/kael_notifications/src/push.rs`
- Create: `crates/kael_notifications/src/action.rs`
- Create: `crates/kael_notifications/src/platform/mac.rs`
- Create: `crates/kael_notifications/src/platform/linux.rs`
- Create: `crates/kael_notifications/src/platform/windows.rs`

#### Public API

```rust
pub struct NotificationCenter { /* ... */ }

impl NotificationCenter {
    pub fn request_authorization(&self, options: AuthorizationOptions) -> Task<Result<bool>>;
    pub fn schedule_local(&self, notification: LocalNotification) -> Result<NotificationId>;
    pub fn cancel(&self, id: &NotificationId);
    pub fn cancel_all(&self);
    pub fn register_for_push(&self) -> Task<Result<PushToken>>;
    pub fn on_received(&self, callback: impl Fn(NotificationEvent) + Send + 'static) -> Subscription;
    pub fn set_badge_count(&self, count: u32);
}

pub struct LocalNotification {
    pub title: String,
    pub body: String,
    pub subtitle: Option<String>,
    pub sound: Option<NotificationSound>,
    pub badge: Option<u32>,
    pub category: Option<String>,
    pub user_info: HashMap<String, String>,
    pub trigger: NotificationTrigger,
    pub attachments: Vec<NotificationAttachment>,
}

pub enum NotificationTrigger {
    Immediate,
    TimeInterval { seconds: f64, repeats: bool },
    Calendar { date_components: DateComponents, repeats: bool },
    Location { region: CircularRegion, on_entry: bool, on_exit: bool },
}

pub struct NotificationCategory {
    pub identifier: String,
    pub actions: Vec<NotificationAction>,
}

pub struct NotificationAction {
    pub identifier: String,
    pub title: String,
    pub options: ActionOptions,
    pub text_input_placeholder: Option<String>,
}

pub enum NotificationEvent {
    Received(NotificationPayload),
    ActionPerformed { notification: NotificationPayload, action_id: String, text_input: Option<String> },
    Dismissed(NotificationPayload),
}

pub struct PushToken(pub Vec<u8>);
```

#### Platform Push Implementation

| Platform | Service | Registration |
|----------|---------|-------------|
| macOS | APNs via `NSApplication.registerForRemoteNotifications` | Returns device token via delegate |
| Linux | Custom WebSocket connection (app provides server) | App-managed token |
| Windows | WNS via `PushNotificationChannelManager` | Returns channel URI |

#### Tasks

- [ ] Create crate scaffold
- [ ] Implement `NotificationCenter` with authorization flow
- [ ] Implement local notification scheduling (all triggers)
- [ ] Implement notification actions and categories
- [ ] Implement macOS push registration (APNs delegate methods)
- [ ] Implement Linux notification via D-Bus `org.freedesktop.Notifications`
- [ ] Implement Windows toast notifications with actions
- [ ] Implement push token handling and event callbacks
- [ ] Implement notification attachments (images, audio)
- [ ] Implement badge count management
- [ ] Write tests: scheduling, cancellation, action dispatch
- [ ] Commit

---

### Crate 8: `kael_share` — System Share Sheet

**Purpose:** Share content (text, URLs, images, files) via system share UI.

**Dependencies:** Platform FFI only

**Files:**
- Create: `crates/kael_share/Cargo.toml`
- Create: `crates/kael_share/src/lib.rs`
- Create: `crates/kael_share/src/platform/mac.rs`
- Create: `crates/kael_share/src/platform/linux.rs`
- Create: `crates/kael_share/src/platform/windows.rs`

#### Public API

```rust
pub struct ShareItem {
    pub text: Option<String>,
    pub url: Option<String>,
    pub image: Option<SharedImage>,
    pub files: Vec<PathBuf>,
    pub subject: Option<String>, // email subject
}

pub struct ShareSheet { /* ... */ }

impl ShareSheet {
    pub fn new(items: Vec<ShareItem>) -> Self;
    pub fn excluded_types(self, types: &[ShareType]) -> Self;
    pub fn show(&self, cx: &mut Window, anchor: Bounds<Pixels>) -> Task<Result<ShareResult>>;
}

pub enum ShareType { Mail, Messages, AirDrop, Clipboard, Social, Print }

pub enum ShareResult {
    Completed { activity_type: String },
    Cancelled,
}

// Receive shared content (register as share target)
pub struct ShareReceiver { /* ... */ }
impl ShareReceiver {
    pub fn register(file_types: &[FileType], callback: impl Fn(Vec<ShareItem>) + Send + 'static);
}
```

#### Tasks

- [ ] Create crate scaffold
- [ ] Implement macOS share sheet via `NSSharingServicePicker`
- [ ] Implement Linux share via `xdg-open` / Portal D-Bus
- [ ] Implement Windows share via `DataTransferManager`
- [ ] Implement share item construction (text, URL, image, file)
- [ ] Implement share result callback
- [ ] Write tests: item construction, type exclusion
- [ ] Commit

---

### Crate 9: `kael_search` — System Search Indexing

**Purpose:** Index app content for system-wide search (Spotlight on macOS).

**Dependencies:** Platform FFI

**Files:**
- Create: `crates/kael_search/Cargo.toml`
- Create: `crates/kael_search/src/lib.rs`
- Create: `crates/kael_search/src/indexer.rs`
- Create: `crates/kael_search/src/platform/mac.rs`
- Create: `crates/kael_search/src/platform/linux.rs`
- Create: `crates/kael_search/src/platform/windows.rs`

#### Public API

```rust
pub struct SearchIndex { /* ... */ }

impl SearchIndex {
    pub fn new(domain: &str) -> Self;
    pub async fn index(&self, items: Vec<SearchableItem>) -> Result<()>;
    pub async fn remove(&self, identifiers: &[&str]) -> Result<()>;
    pub async fn remove_all(&self) -> Result<()>;
    pub fn on_continue_activity(&self, callback: impl Fn(String) + Send + 'static) -> Subscription;
}

pub struct SearchableItem {
    pub unique_identifier: String,
    pub domain_identifier: Option<String>,
    pub title: String,
    pub content_description: Option<String>,
    pub thumbnail: Option<SharedImage>,
    pub keywords: Vec<String>,
    pub attributes: HashMap<String, SearchAttributeValue>,
    pub expiration_date: Option<SystemTime>,
}

pub enum SearchAttributeValue {
    String(String),
    Number(f64),
    Date(SystemTime),
    Boolean(bool),
}
```

#### Platform Implementation

| Platform | Index Engine | Activation |
|----------|-------------|------------|
| macOS | Core Spotlight (`CSSearchableIndex`) | `NSUserActivity` continuation |
| Linux | Tracker3 via SPARQL endpoint | D-Bus activation |
| Windows | Windows Search protocol handler | URI scheme activation |

#### Tasks

- [ ] Create crate scaffold
- [ ] Implement macOS Core Spotlight indexing (`CSSearchableItem`, `CSSearchableIndex`)
- [ ] Implement macOS continuation (respond to Spotlight result tap)
- [ ] Implement Linux Tracker3 integration
- [ ] Implement Windows Search protocol handler
- [ ] Implement batch indexing with rate limiting
- [ ] Write tests: index/remove round-trip, attribute types
- [ ] Commit

---

### Crate 10: `kael_automation` — Scripting & Automation

**Purpose:** Make apps scriptable via AppleScript (macOS), D-Bus (Linux), COM (Windows).

**Dependencies:** Platform FFI, `serde`

**Files:**
- Create: `crates/kael_automation/Cargo.toml`
- Create: `crates/kael_automation/src/lib.rs`
- Create: `crates/kael_automation/src/command.rs`
- Create: `crates/kael_automation/src/scripting.rs`
- Create: `crates/kael_automation/src/platform/mac.rs`
- Create: `crates/kael_automation/src/platform/linux.rs`
- Create: `crates/kael_automation/src/platform/windows.rs`

#### Public API

```rust
pub trait Scriptable: Send + Sync + 'static {
    fn scripting_commands() -> Vec<ScriptCommand>;
    fn handle_command(&self, command: &str, args: &ScriptArgs) -> Result<ScriptValue>;
}

pub struct ScriptCommand {
    pub name: String,
    pub description: String,
    pub parameters: Vec<ScriptParameter>,
    pub return_type: ScriptType,
}

pub struct ScriptParameter {
    pub name: String,
    pub param_type: ScriptType,
    pub optional: bool,
}

pub enum ScriptType { String, Number, Boolean, List, Record, Reference }

pub enum ScriptValue {
    String(String),
    Number(f64),
    Boolean(bool),
    List(Vec<ScriptValue>),
    Record(HashMap<String, ScriptValue>),
    Null,
}

pub struct ScriptArgs { /* access parameters by name */ }
impl ScriptArgs {
    pub fn get<T: FromScriptValue>(&self, name: &str) -> Option<T>;
}

// Execute scripts from within the app
pub struct ScriptEngine { /* ... */ }
impl ScriptEngine {
    pub async fn execute_applescript(&self, source: &str) -> Result<ScriptValue>;
    pub async fn execute_shell(&self, command: &str) -> Result<String>;
}

// Register app as scriptable
pub fn register_scriptable<T: Scriptable>(app: &App, handler: T);
```

#### Platform Implementation

| Platform | Mechanism | Discovery |
|----------|-----------|-----------|
| macOS | Apple Events + sdef (scripting definition) | Script Editor, Automator, Shortcuts |
| Linux | D-Bus interface with introspection XML | D-Bus tools, KDE/GNOME automation |
| Windows | COM IDispatch automation | PowerShell, VBScript |

On macOS: generate `.sdef` file at build time from `ScriptCommand` definitions. Register Apple Event handlers via `NSAppleEventManager`.

#### Tasks

- [ ] Create crate scaffold
- [ ] Define `Scriptable` trait and command registration system
- [ ] Implement macOS Apple Events handler (receive events, dispatch to trait)
- [ ] Implement `.sdef` file generation from ScriptCommand definitions
- [ ] Implement macOS `ScriptEngine::execute_applescript`
- [ ] Implement Linux D-Bus interface exposure (introspectable)
- [ ] Implement Windows COM automation object
- [ ] Implement shell script execution (cross-platform)
- [ ] Write tests: command dispatch, argument parsing, return values
- [ ] Write example: scriptable text editor (get/set document text via AppleScript)
- [ ] Commit

---

## Layer 3: Device Services

---

### Crate 11: `kael_location` — Geolocation

**Purpose:** Current location, continuous tracking, geofencing, heading.

**Dependencies:** Platform FFI

**Files:**
- Create: `crates/kael_location/Cargo.toml`
- Create: `crates/kael_location/src/lib.rs`
- Create: `crates/kael_location/src/manager.rs`
- Create: `crates/kael_location/src/region.rs`
- Create: `crates/kael_location/src/platform/mac.rs`
- Create: `crates/kael_location/src/platform/linux.rs`
- Create: `crates/kael_location/src/platform/windows.rs`

#### Public API

```rust
pub struct LocationManager { /* ... */ }

impl LocationManager {
    pub fn new() -> Self;
    pub fn authorization_status(&self) -> AuthorizationStatus;
    pub fn request_authorization(&self, level: AuthorizationLevel) -> Task<Result<AuthorizationStatus>>;
    pub fn request_location(&self) -> Task<Result<Location>>;
    pub fn start_updates(&self, config: LocationConfig) -> LocationStream;
    pub fn stop_updates(&self);
    pub fn start_monitoring_region(&self, region: Region) -> Result<()>;
    pub fn stop_monitoring_region(&self, identifier: &str);
    pub fn start_heading_updates(&self) -> HeadingStream;
}

pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: f64,
    pub horizontal_accuracy: f64,
    pub vertical_accuracy: f64,
    pub speed: f64,
    pub course: f64,
    pub timestamp: SystemTime,
}

pub struct LocationConfig {
    pub desired_accuracy: Accuracy,
    pub distance_filter: f64, // meters, 0 = all updates
}

pub enum Accuracy { Best, NearestTenMeters, HundredMeters, Kilometer, ThreeKilometers }

pub struct Region {
    pub identifier: String,
    pub center: (f64, f64), // lat, lon
    pub radius: f64,        // meters
    pub notify_on_entry: bool,
    pub notify_on_exit: bool,
}

pub enum RegionEvent { Enter(Region), Exit(Region) }

pub enum AuthorizationStatus { NotDetermined, Denied, AuthorizedAlways, AuthorizedWhenInUse }
pub enum AuthorizationLevel { WhenInUse, Always }
```

#### Platform Implementation

| Platform | Backend | Geofencing |
|----------|---------|-----------|
| macOS | Core Location (`CLLocationManager`) | `CLCircularRegion` monitoring |
| Linux | GeoClue2 via D-Bus (`org.freedesktop.GeoClue2`) | Software-based (poll + check) |
| Windows | `Windows.Devices.Geolocation.Geolocator` | `Geofence` API |

#### Tasks

- [ ] Create crate scaffold
- [ ] Implement macOS backend: CLLocationManager delegate via objc2
- [ ] Implement authorization request flow (macOS)
- [ ] Implement single-shot location request
- [ ] Implement continuous location updates with distance filter
- [ ] Implement geofence monitoring (macOS CLCircularRegion)
- [ ] Implement heading updates (macOS compass)
- [ ] Implement Linux backend: GeoClue2 D-Bus client
- [ ] Implement Windows backend: Geolocator API
- [ ] Implement software geofencing for Linux (periodic check)
- [ ] Write tests: authorization state machine, region events
- [ ] Commit

---

### Crate 12: `kael_bluetooth` — Bluetooth LE

**Purpose:** Scan, connect, and communicate with BLE peripherals; advertise as peripheral.

**Dependencies:** `btleplug` (cross-platform BLE, uses CoreBluetooth/BlueZ/WinRT internally)

**Files:**
- Create: `crates/kael_bluetooth/Cargo.toml`
- Create: `crates/kael_bluetooth/src/lib.rs`
- Create: `crates/kael_bluetooth/src/central.rs`
- Create: `crates/kael_bluetooth/src/peripheral.rs`
- Create: `crates/kael_bluetooth/src/service.rs`

#### Public API

```rust
pub struct BluetoothCentral { /* ... */ }

impl BluetoothCentral {
    pub async fn new() -> Result<Self>;
    pub fn state(&self) -> BluetoothState;
    pub fn on_state_change(&self, callback: impl Fn(BluetoothState) + Send + 'static) -> Subscription;
    pub async fn scan(&self, filter: ScanFilter) -> Result<ScanStream>;
    pub async fn stop_scan(&self);
    pub async fn connect(&self, peripheral: &DiscoveredPeripheral) -> Result<ConnectedPeripheral>;
}

pub struct ConnectedPeripheral { /* ... */ }

impl ConnectedPeripheral {
    pub async fn discover_services(&self) -> Result<Vec<Service>>;
    pub async fn read(&self, characteristic: &Characteristic) -> Result<Vec<u8>>;
    pub async fn write(&self, characteristic: &Characteristic, data: &[u8], with_response: bool) -> Result<()>;
    pub async fn subscribe(&self, characteristic: &Characteristic) -> Result<NotificationStream>;
    pub async fn unsubscribe(&self, characteristic: &Characteristic) -> Result<()>;
    pub async fn disconnect(&self);
    pub fn rssi(&self) -> Task<Result<i16>>;
}

pub struct ScanFilter {
    pub services: Vec<Uuid>,
    pub allow_duplicates: bool,
}

pub struct DiscoveredPeripheral {
    pub id: PeripheralId,
    pub name: Option<String>,
    pub rssi: i16,
    pub advertisement_data: AdvertisementData,
}

pub struct Service {
    pub uuid: Uuid,
    pub characteristics: Vec<Characteristic>,
}

pub struct Characteristic {
    pub uuid: Uuid,
    pub properties: CharacteristicProperties,
    pub descriptors: Vec<Descriptor>,
}

pub enum BluetoothState { Unknown, Resetting, Unsupported, Unauthorized, PoweredOff, PoweredOn }

// Peripheral mode (advertise)
pub struct BluetoothPeripheral { /* ... */ }
impl BluetoothPeripheral {
    pub async fn new() -> Result<Self>;
    pub fn start_advertising(&self, advertisement: Advertisement) -> Result<()>;
    pub fn stop_advertising(&self);
    pub fn add_service(&self, service: ServiceDefinition) -> Result<()>;
    pub fn on_read_request(&self, callback: impl Fn(ReadRequest) -> Vec<u8> + Send + 'static);
    pub fn on_write_request(&self, callback: impl Fn(WriteRequest) + Send + 'static);
}
```

#### Implementation

Use `btleplug` which already provides cross-platform BLE via:
- macOS: CoreBluetooth
- Linux: BlueZ D-Bus
- Windows: WinRT Bluetooth APIs

We wrap btleplug with our own types for consistency with Kael's API style and to add peripheral-mode support (btleplug is central-only; peripheral mode needs direct platform FFI).

#### Tasks

- [ ] Create crate scaffold
- [ ] Implement `BluetoothCentral` wrapping btleplug
- [ ] Implement scan with filtering
- [ ] Implement connect/disconnect lifecycle
- [ ] Implement service/characteristic discovery
- [ ] Implement read/write/subscribe operations
- [ ] Implement peripheral mode for macOS (CBPeripheralManager via objc2)
- [ ] Implement peripheral mode for Linux (BlueZ GATT server via D-Bus)
- [ ] Implement peripheral mode for Windows (GattServiceProvider)
- [ ] Write tests: scan filter, connection state machine
- [ ] Write example: BLE device scanner
- [ ] Commit

---

### Crate 13: `kael_maps` — Map Display & Geocoding

**Purpose:** Interactive map element with annotations, geocoding, and directions.

**Dependencies:** Platform map views (macOS MapKit), tile renderer (Linux/Windows)

**Files:**
- Create: `crates/kael_maps/Cargo.toml`
- Create: `crates/kael_maps/src/lib.rs`
- Create: `crates/kael_maps/src/map_view.rs`
- Create: `crates/kael_maps/src/annotation.rs`
- Create: `crates/kael_maps/src/geocoder.rs`
- Create: `crates/kael_maps/src/directions.rs`
- Create: `crates/kael_maps/src/tile_renderer.rs` (cross-platform fallback)
- Create: `crates/kael_maps/src/platform/mac.rs`

#### Public API

```rust
pub struct MapView { /* ... */ }

impl MapView {
    pub fn new() -> Self;
    pub fn region(self, region: MapRegion) -> Self;
    pub fn map_type(self, map_type: MapType) -> Self;
    pub fn shows_user_location(self, show: bool) -> Self;
    pub fn annotations(self, annotations: Vec<MapAnnotation>) -> Self;
    pub fn overlays(self, overlays: Vec<MapOverlay>) -> Self;
    pub fn on_region_change(self, callback: impl Fn(MapRegion) + Send + 'static) -> Self;
    pub fn on_annotation_selected(self, callback: impl Fn(&MapAnnotation) + Send + 'static) -> Self;
}

impl IntoElement for MapView { /* ... */ }

pub struct MapRegion {
    pub center: Coordinate,
    pub span: CoordinateSpan,
}

pub struct Coordinate {
    pub latitude: f64,
    pub longitude: f64,
}

pub struct CoordinateSpan {
    pub latitude_delta: f64,
    pub longitude_delta: f64,
}

pub enum MapType { Standard, Satellite, Hybrid, MutedStandard }

pub struct MapAnnotation {
    pub id: String,
    pub coordinate: Coordinate,
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub color: Option<Hsla>,
    pub image: Option<SharedImage>,
}

pub enum MapOverlay {
    Polyline { coordinates: Vec<Coordinate>, color: Hsla, width: f32 },
    Polygon { coordinates: Vec<Coordinate>, fill_color: Hsla, stroke_color: Hsla },
    Circle { center: Coordinate, radius: f64, fill_color: Hsla, stroke_color: Hsla },
}

// Geocoding
pub struct Geocoder { /* ... */ }
impl Geocoder {
    pub async fn geocode(&self, address: &str) -> Result<Vec<Placemark>>;
    pub async fn reverse_geocode(&self, coordinate: Coordinate) -> Result<Vec<Placemark>>;
}

pub struct Placemark {
    pub coordinate: Coordinate,
    pub name: Option<String>,
    pub street: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub country: Option<String>,
    pub postal_code: Option<String>,
}

// Directions
pub struct DirectionsRequest {
    pub origin: Coordinate,
    pub destination: Coordinate,
    pub transport_type: TransportType,
}

pub enum TransportType { Automobile, Walking, Transit }

pub struct Route {
    pub distance: f64,          // meters
    pub expected_travel_time: Duration,
    pub polyline: Vec<Coordinate>,
    pub steps: Vec<RouteStep>,
}
```

#### Platform Implementation

| Platform | Map Rendering | Geocoding | Directions |
|----------|--------------|-----------|------------|
| macOS | MapKit (`MKMapView` embedded as NSView) | `CLGeocoder` | `MKDirections` |
| Linux | Custom tile renderer (OpenStreetMap raster tiles) | Nominatim HTTP API | OSRM HTTP API |
| Windows | Custom tile renderer (same as Linux) | Windows.Services.Maps | Same as Linux |

The macOS backend embeds a native `MKMapView` as a platform view within the kael window (similar to how WebView is embedded). Linux/Windows use a custom tile-based renderer that downloads and caches OpenStreetMap tiles, rendering them to GPU textures.

#### Tasks

- [ ] Create crate scaffold
- [ ] Implement `MapView` kael Element shell
- [ ] Implement macOS backend: embed MKMapView via NSView
- [ ] Implement macOS annotations and overlays
- [ ] Implement macOS geocoding (CLGeocoder bridge)
- [ ] Implement macOS directions (MKDirections)
- [ ] Implement cross-platform tile renderer (download OSM tiles, cache, composite)
- [ ] Implement tile renderer pan/zoom gestures
- [ ] Implement annotation rendering on tile map
- [ ] Implement overlay rendering (polyline, polygon, circle)
- [ ] Implement geocoding via Nominatim API (Linux/Windows)
- [ ] Implement directions via OSRM API (Linux/Windows)
- [ ] Write tests: region calculations, geocoding parse
- [ ] Write example: map with search + pin drop
- [ ] Commit

---

## Layer 4: Intelligence

---

### Crate 14: `kael_ml` — On-Device Machine Learning

**Purpose:** Load and run ML models for inference (classification, detection, embeddings).

**Dependencies:** `onnxruntime` (C API), macOS Core ML via FFI

**Files:**
- Create: `crates/kael_ml/Cargo.toml`
- Create: `crates/kael_ml/src/lib.rs`
- Create: `crates/kael_ml/src/model.rs`
- Create: `crates/kael_ml/src/tensor.rs`
- Create: `crates/kael_ml/src/tasks/mod.rs`
- Create: `crates/kael_ml/src/tasks/classification.rs`
- Create: `crates/kael_ml/src/tasks/detection.rs`
- Create: `crates/kael_ml/src/tasks/embedding.rs`
- Create: `crates/kael_ml/src/platform/mac.rs`
- Create: `crates/kael_ml/src/platform/onnx.rs`
- Create: `crates/kael_ml/onnxruntime-sys/` (C binding crate)

#### Public API

```rust
pub struct Model { /* ... */ }

impl Model {
    pub async fn load(path: impl AsRef<Path>) -> Result<Self>;
    pub async fn load_from_memory(data: &[u8], format: ModelFormat) -> Result<Self>;
    pub fn input_info(&self) -> Vec<TensorInfo>;
    pub fn output_info(&self) -> Vec<TensorInfo>;
    pub async fn predict(&self, inputs: &[Tensor]) -> Result<Vec<Tensor>>;
}

pub enum ModelFormat {
    CoreML,    // .mlmodel / .mlpackage (macOS only)
    Onnx,     // .onnx (cross-platform)
}

pub struct Tensor {
    pub shape: Vec<usize>,
    pub data: TensorData,
}

pub enum TensorData {
    Float32(Vec<f32>),
    Float16(Vec<u16>),
    Int32(Vec<i32>),
    Int64(Vec<i64>),
    UInt8(Vec<u8>),
    String(Vec<String>),
}

pub struct TensorInfo {
    pub name: String,
    pub shape: Vec<Option<usize>>, // None = dynamic dimension
    pub data_type: DataType,
}

// High-level task APIs (convenience wrappers)
pub struct ImageClassifier { /* ... */ }
impl ImageClassifier {
    pub async fn load(model: Model) -> Result<Self>;
    pub async fn classify(&self, image: &RenderImage) -> Result<Vec<Classification>>;
}

pub struct Classification {
    pub label: String,
    pub confidence: f32,
}

pub struct ObjectDetector { /* ... */ }
impl ObjectDetector {
    pub async fn load(model: Model) -> Result<Self>;
    pub async fn detect(&self, image: &RenderImage) -> Result<Vec<Detection>>;
}

pub struct Detection {
    pub label: String,
    pub confidence: f32,
    pub bounding_box: Bounds<f32>, // normalized 0-1
}

pub struct TextEmbedder { /* ... */ }
impl TextEmbedder {
    pub async fn load(model: Model) -> Result<Self>;
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>>;
    pub async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
}
```

#### Platform Implementation

| Platform | Runtime | Acceleration |
|----------|---------|-------------|
| macOS | Core ML (`MLModel`) via objc2 FFI | Apple Neural Engine + Metal |
| Linux | ONNX Runtime C API | CPU (default), CUDA (optional) |
| Windows | ONNX Runtime C API | CPU, DirectML (optional) |

On macOS, prefer Core ML for models in `.mlmodel` format (uses ANE/GPU automatically). For `.onnx` models on macOS, either convert to Core ML at load time or use ONNX Runtime with CoreML execution provider.

#### Tasks

- [ ] Create crate scaffold and onnxruntime-sys binding crate
- [ ] Write build.rs for ONNX Runtime download and linking
- [ ] Implement `Tensor` and tensor data conversions
- [ ] Implement `Model::load` for ONNX format (cross-platform baseline)
- [ ] Implement `Model::predict` (feed inputs, collect outputs)
- [ ] Implement macOS Core ML backend (load .mlmodel, run inference)
- [ ] Implement `ImageClassifier` high-level API
- [ ] Implement `ObjectDetector` high-level API (with NMS post-processing)
- [ ] Implement `TextEmbedder` high-level API
- [ ] Implement image preprocessing helpers (resize, normalize, tensor conversion)
- [ ] Write tests: tensor shape validation, model loading
- [ ] Write example: image classification demo
- [ ] Commit

---

### Crate 15: `kael_nlp` — Natural Language Processing

**Purpose:** Language detection, tokenization, POS tagging, NER, sentiment analysis.

**Dependencies:** macOS NaturalLanguage.framework, `whatlang` (language detection), `rust-tokenizers`

**Files:**
- Create: `crates/kael_nlp/Cargo.toml`
- Create: `crates/kael_nlp/src/lib.rs`
- Create: `crates/kael_nlp/src/language.rs`
- Create: `crates/kael_nlp/src/tokenizer.rs`
- Create: `crates/kael_nlp/src/tagger.rs`
- Create: `crates/kael_nlp/src/sentiment.rs`
- Create: `crates/kael_nlp/src/platform/mac.rs`
- Create: `crates/kael_nlp/src/portable.rs`

#### Public API

```rust
pub struct LanguageRecognizer { /* ... */ }
impl LanguageRecognizer {
    pub fn detect(text: &str) -> Option<Language>;
    pub fn detect_with_probabilities(text: &str) -> Vec<(Language, f64)>;
}

pub struct Tokenizer { /* ... */ }
impl Tokenizer {
    pub fn new(language: Language) -> Self;
    pub fn tokenize(&self, text: &str) -> Vec<Token>;
    pub fn sentences(&self, text: &str) -> Vec<&str>;
    pub fn words(&self, text: &str) -> Vec<&str>;
    pub fn paragraphs(&self, text: &str) -> Vec<&str>;
}

pub struct Token {
    pub text: String,
    pub range: Range<usize>,
    pub kind: TokenKind,
}

pub enum TokenKind { Word, Punctuation, Whitespace, Other }

pub struct Tagger { /* ... */ }
impl Tagger {
    pub fn new(language: Language, scheme: TagScheme) -> Self;
    pub fn tag(&self, text: &str) -> Vec<TaggedToken>;
}

pub enum TagScheme { PartOfSpeech, NamedEntity, Lemma }

pub struct TaggedToken {
    pub token: Token,
    pub tag: String,       // e.g., "Noun", "Verb", "PersonalName", "PlaceName"
    pub confidence: f64,
}

pub struct SentimentAnalyzer { /* ... */ }
impl SentimentAnalyzer {
    pub fn new(language: Language) -> Self;
    pub fn analyze(&self, text: &str) -> Sentiment;
}

pub struct Sentiment {
    pub score: f64,    // -1.0 (negative) to 1.0 (positive)
    pub label: SentimentLabel,
}

pub enum SentimentLabel { Positive, Negative, Neutral }

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    English, Spanish, French, German, Italian, Portuguese,
    Chinese, Japanese, Korean, Russian, Arabic, Hindi,
    // ... all ISO 639-1 languages
    Unknown,
}
```

#### Platform Implementation

| Platform | Tokenizer | POS/NER | Sentiment |
|----------|-----------|---------|-----------|
| macOS | NaturalLanguage.framework (`NLTokenizer`) | `NLTagger` | `NLTagger` with `.sentimentScore` |
| Linux | `unicode-segmentation` + custom rules | Lightweight rule-based or kael_ml model | kael_ml model |
| Windows | Same as Linux | Same as Linux | Same as Linux |

macOS provides excellent NLP via `NaturalLanguage.framework` — we bridge it directly. For Linux/Windows, provide a portable implementation using:
- `whatlang` for language detection
- `unicode-segmentation` for tokenization
- Rule-based POS tagger for common languages, or optional ML model via `kael_ml`

#### Tasks

- [ ] Create crate scaffold
- [ ] Implement `LanguageRecognizer` (macOS: NLLanguageRecognizer, portable: whatlang)
- [ ] Implement `Tokenizer` (macOS: NLTokenizer, portable: unicode-segmentation)
- [ ] Implement sentence/word/paragraph segmentation
- [ ] Implement `Tagger` for macOS (NLTagger with POS and NER schemes)
- [ ] Implement portable POS tagger (rule-based for English)
- [ ] Implement `SentimentAnalyzer` (macOS: NLTagger sentimentScore)
- [ ] Implement portable sentiment (lexicon-based VADER-style)
- [ ] Write tests: language detection accuracy, tokenization edge cases
- [ ] Commit

---

## Layer 5: Distribution

---

### Crate 16: `kael_licensing` — In-App Purchase & License Management

**Purpose:** App Store purchases on macOS, custom license key validation cross-platform.

**Dependencies:** Platform FFI (StoreKit 2), `ring` (crypto for license validation)

**Files:**
- Create: `crates/kael_licensing/Cargo.toml`
- Create: `crates/kael_licensing/src/lib.rs`
- Create: `crates/kael_licensing/src/store.rs`
- Create: `crates/kael_licensing/src/license_key.rs`
- Create: `crates/kael_licensing/src/trial.rs`
- Create: `crates/kael_licensing/src/platform/mac.rs`
- Create: `crates/kael_licensing/src/platform/windows.rs`

#### Public API

```rust
// App Store (macOS App Store, Microsoft Store)
pub struct Store { /* ... */ }
impl Store {
    pub async fn products(&self, ids: &[&str]) -> Result<Vec<Product>>;
    pub async fn purchase(&self, product: &Product) -> Result<Transaction>;
    pub async fn current_entitlements(&self) -> Result<Vec<Transaction>>;
    pub async fn restore_purchases(&self) -> Result<Vec<Transaction>>;
    pub fn on_transaction_update(&self, callback: impl Fn(Transaction) + Send + 'static) -> Subscription;
}

pub struct Product {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub price: Decimal,
    pub price_formatted: String,
    pub product_type: ProductType,
    pub subscription: Option<SubscriptionInfo>,
}

pub enum ProductType { Consumable, NonConsumable, AutoRenewable, NonRenewable }

pub struct Transaction {
    pub id: String,
    pub product_id: String,
    pub purchase_date: SystemTime,
    pub expiration_date: Option<SystemTime>,
    pub is_upgraded: bool,
}

// Custom License Key (for non-App Store distribution)
pub struct LicenseManager { /* ... */ }
impl LicenseManager {
    pub fn new(config: LicenseConfig) -> Self;
    pub fn activate(&self, key: &str) -> Task<Result<License>>;
    pub fn deactivate(&self) -> Task<Result<()>>;
    pub fn current_license(&self) -> Option<&License>;
    pub fn validate(&self) -> Task<Result<LicenseStatus>>;
}

pub struct LicenseConfig {
    pub public_key: &'static str,          // Ed25519 public key for offline validation
    pub validation_url: Option<String>,     // Online validation endpoint
    pub grace_period: Duration,             // How long to allow offline after last check
    pub hardware_binding: bool,             // Bind to machine
}

pub struct License {
    pub key: String,
    pub email: Option<String>,
    pub plan: String,
    pub seats: u32,
    pub valid_until: Option<SystemTime>,
    pub features: HashSet<String>,
}

pub enum LicenseStatus { Valid, Expired, Revoked, InvalidKey, NetworkError }

// Trial management
pub struct TrialManager { /* ... */ }
impl TrialManager {
    pub fn new(config: TrialConfig) -> Self;
    pub fn remaining_days(&self) -> u32;
    pub fn is_expired(&self) -> bool;
    pub fn start_trial(&self);
}

pub struct TrialConfig {
    pub duration_days: u32,
    pub feature_limited: bool,   // Limit features vs time-limit full app
}
```

#### Platform Implementation

| Platform | App Store | License Validation |
|----------|-----------|-------------------|
| macOS | StoreKit 2 (`Product`, `Transaction`) via Swift/objc2 | Ed25519 signature + hardware ID (`IOPlatformUUID`) |
| Linux | N/A | Ed25519 signature + hardware ID (`/etc/machine-id`) |
| Windows | Microsoft Store (`StoreContext`) | Ed25519 signature + hardware ID (WMI `Win32_ComputerSystemProduct.UUID`) |

License key format: `XXXXX-XXXXX-XXXXX-XXXXX-XXXXX` encoding a signed payload (product ID, expiration, features, hardware hash). Validated offline using Ed25519. Optional online check for revocation.

#### Tasks

- [ ] Create crate scaffold
- [ ] Implement `LicenseConfig` and key format (Ed25519 signed payload)
- [ ] Implement offline license validation (verify signature, check expiration)
- [ ] Implement hardware ID collection per platform
- [ ] Implement online validation endpoint client
- [ ] Implement `LicenseManager` (activate, deactivate, persist to secure storage)
- [ ] Implement `TrialManager` (first-launch detection, countdown)
- [ ] Implement macOS StoreKit 2 bridge (`Product.products(for:)`, `purchase()`)
- [ ] Implement macOS transaction observation and entitlement checking
- [ ] Implement Windows Microsoft Store bridge (if targeting MS Store)
- [ ] Write tests: key validation, expiration, hardware binding
- [ ] Commit

---

### Crate 17: `kael_updater` — Auto-Update & Code Signing

**Purpose:** Check for updates, download, verify, and apply with minimal user friction.

**Dependencies:** `ring` (signature verification), `http_client` (existing), `flate2`/`tar` (archive)

**Files:**
- Create: `crates/kael_updater/Cargo.toml`
- Create: `crates/kael_updater/src/lib.rs`
- Create: `crates/kael_updater/src/appcast.rs`
- Create: `crates/kael_updater/src/download.rs`
- Create: `crates/kael_updater/src/verifier.rs`
- Create: `crates/kael_updater/src/installer.rs`
- Create: `crates/kael_updater/src/platform/mac.rs`
- Create: `crates/kael_updater/src/platform/linux.rs`
- Create: `crates/kael_updater/src/platform/windows.rs`

#### Public API

```rust
pub struct Updater { /* ... */ }

impl Updater {
    pub fn new(config: UpdaterConfig) -> Self;
    pub async fn check_for_update(&self) -> Result<Option<UpdateInfo>>;
    pub async fn download(&self, update: &UpdateInfo, progress: impl Fn(DownloadProgress) + Send + 'static) -> Result<PathBuf>;
    pub async fn verify(&self, path: &Path, update: &UpdateInfo) -> Result<()>;
    pub async fn install_and_restart(&self, path: &Path) -> Result<()>;
    pub fn set_channel(&self, channel: UpdateChannel);

    // Convenience: check + prompt + download + install
    pub async fn check_and_prompt(&self, cx: &mut Window) -> Result<UpdateAction>;
}

pub struct UpdaterConfig {
    pub appcast_url: String,
    pub public_key: &'static str,       // Ed25519 for signature verification
    pub current_version: &'static str,
    pub channel: UpdateChannel,
    pub check_interval: Duration,       // default 24h
    pub automatic_download: bool,       // download in background
}

pub enum UpdateChannel { Stable, Beta, Nightly }

pub struct UpdateInfo {
    pub version: String,
    pub release_notes: String,
    pub release_notes_html: Option<String>,
    pub download_url: String,
    pub download_size: u64,
    pub signature: String,
    pub minimum_os_version: Option<String>,
    pub critical: bool,
}

pub struct DownloadProgress {
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
}

pub enum UpdateAction { Installed, UserDeclined, AlreadyUpToDate, Error(String) }
```

#### Appcast Format (Sparkle-compatible XML)

```xml
<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
  <channel>
    <item>
      <title>Version 2.0.1</title>
      <sparkle:version>2.0.1</sparkle:version>
      <sparkle:channel>stable</sparkle:channel>
      <description><![CDATA[<h2>Bug fixes</h2><ul><li>Fixed crash on startup</li></ul>]]></description>
      <enclosure url="https://example.com/app-2.0.1.tar.gz"
                 sparkle:edSignature="base64signature..."
                 length="15728640"
                 type="application/octet-stream"/>
      <sparkle:minimumSystemVersion>13.0</sparkle:minimumSystemVersion>
    </item>
  </channel>
</rss>
```

#### Platform Installation

| Platform | Update Mechanism | Restart |
|----------|-----------------|---------|
| macOS | Replace .app bundle (atomic swap via `renameat2` or temp + rename) | `NSWorkspace.open` new app, then `exit(0)` |
| Linux | Replace AppImage or binary in place | `execvp` to restart self |
| Windows | Extract to temp, launch updater helper that waits for process exit then copies | Helper process |

#### Tasks

- [ ] Create crate scaffold
- [ ] Implement appcast XML parser (Sparkle-compatible)
- [ ] Implement version comparison (semver)
- [ ] Implement download with progress reporting
- [ ] Implement Ed25519 signature verification of downloaded archive
- [ ] Implement macOS installer (atomic .app bundle replacement)
- [ ] Implement Linux installer (replace binary, handle AppImage)
- [ ] Implement Windows installer (helper process pattern)
- [ ] Implement restart mechanism per platform
- [ ] Implement periodic background check (timer-based)
- [ ] Implement UI prompt for update (using kael dialog)
- [ ] Implement update channels (stable/beta/nightly filtering)
- [ ] Write tests: appcast parsing, version comparison, signature verification
- [ ] Commit

---

### Crate 18: `kael_sync` — Cross-Device Data Sync

**Purpose:** Sync app data across devices using CloudKit (macOS) or custom backend.

**Dependencies:** `kael_storage`, platform FFI (CloudKit), `serde`

**Files:**
- Create: `crates/kael_sync/Cargo.toml`
- Create: `crates/kael_sync/src/lib.rs`
- Create: `crates/kael_sync/src/engine.rs`
- Create: `crates/kael_sync/src/conflict.rs`
- Create: `crates/kael_sync/src/change.rs`
- Create: `crates/kael_sync/src/backend/mod.rs`
- Create: `crates/kael_sync/src/backend/cloudkit.rs`
- Create: `crates/kael_sync/src/backend/custom.rs`

#### Public API

```rust
pub trait Syncable: Serialize + DeserializeOwned + Clone + Send + Sync + 'static {
    type Id: Clone + Eq + Hash + Serialize + DeserializeOwned + Send + Sync;
    fn id(&self) -> &Self::Id;
    fn updated_at(&self) -> SystemTime;
}

pub struct SyncEngine<T: Syncable> { /* ... */ }

impl<T: Syncable> SyncEngine<T> {
    pub fn new(config: SyncConfig, backend: Box<dyn SyncBackend<T>>) -> Self;
    pub async fn push(&self, items: &[T]) -> Result<()>;
    pub async fn pull(&self) -> Result<Vec<SyncChange<T>>>;
    pub async fn sync(&self) -> Result<SyncResult<T>>;
    pub fn on_remote_change(&self, callback: impl Fn(Vec<SyncChange<T>>) + Send + 'static) -> Subscription;
    pub fn set_conflict_resolver(&self, resolver: Box<dyn ConflictResolver<T>>);
}

pub enum SyncChange<T> {
    Created(T),
    Updated(T),
    Deleted(<T as Syncable>::Id),
}

pub struct SyncResult<T: Syncable> {
    pub pushed: usize,
    pub pulled: Vec<SyncChange<T>>,
    pub conflicts: Vec<Conflict<T>>,
}

pub struct Conflict<T: Syncable> {
    pub local: T,
    pub remote: T,
}

pub trait ConflictResolver<T: Syncable>: Send + Sync {
    fn resolve(&self, conflict: &Conflict<T>) -> Resolution<T>;
}

pub enum Resolution<T> {
    KeepLocal,
    KeepRemote,
    Merge(T),
}

pub struct SyncConfig {
    pub container_id: String,
    pub auto_sync_interval: Option<Duration>,
    pub conflict_strategy: ConflictStrategy,
}

pub enum ConflictStrategy { LastWriterWins, AlwaysAsk, Custom }

// Sync backends
pub trait SyncBackend<T: Syncable>: Send + Sync {
    fn push(&self, items: &[T]) -> Task<Result<()>>;
    fn pull(&self, since: Option<SystemTime>) -> Task<Result<Vec<SyncChange<T>>>>;
    fn subscribe(&self, callback: Box<dyn Fn(Vec<SyncChange<T>>) + Send>) -> Result<Subscription>;
}

// Built-in backends
pub fn cloudkit_backend<T: Syncable>(container: &str) -> Box<dyn SyncBackend<T>>; // macOS only
pub fn http_backend<T: Syncable>(config: HttpSyncConfig) -> Box<dyn SyncBackend<T>>; // cross-platform
```

#### Platform Implementation

| Platform | Zero-Config Backend | Push Notifications for Sync |
|----------|--------------------|-----------------------------|
| macOS | CloudKit (`CKContainer`, `CKRecord`, `CKSubscription`) | `CKNotification` (silent push) |
| Linux | HTTP backend (app provides server) | WebSocket for real-time |
| Windows | HTTP backend (same) | WebSocket for real-time |

CloudKit provides free, zero-configuration sync for macOS apps distributed via the App Store. For non-App Store and cross-platform, the HTTP backend talks to a user-provided server (we provide a reference server implementation in a separate repo).

#### Tasks

- [ ] Create crate scaffold
- [ ] Define `Syncable` trait and `SyncEngine` core
- [ ] Implement change tracking (local changelog with timestamps)
- [ ] Implement conflict detection and resolution strategies
- [ ] Implement `http_backend` (REST API client: POST changes, GET since timestamp)
- [ ] Implement WebSocket subscription for real-time push (http backend)
- [ ] Implement macOS CloudKit backend (CKContainer, CKRecord CRUD)
- [ ] Implement CloudKit subscription for remote change notifications
- [ ] Implement CloudKit ↔ Rust type serialization (CKRecord fields)
- [ ] Implement automatic sync scheduling (timer + connectivity check)
- [ ] Implement offline queue (persist pending changes, retry on connectivity)
- [ ] Write tests: push/pull round-trip, conflict resolution, offline queue
- [ ] Write example: synced notes app (create/edit/delete notes across devices)
- [ ] Commit

---

## Integration: Main Kael Crate Feature Flags

After all crates are built, add feature flags to the main `kael` crate:

```toml
# crates/kael/Cargo.toml
[features]
storage = ["dep:kael_storage"]
icons = ["dep:kael_icons"]
diagnostics = ["dep:kael_diagnostics"]
audio = ["dep:kael_audio"]
pdf = ["dep:kael_pdf"]
document = ["dep:kael_document"]
notifications-full = ["dep:kael_notifications"]
share = ["dep:kael_share"]
search = ["dep:kael_search"]
automation = ["dep:kael_automation"]
location = ["dep:kael_location"]
bluetooth = ["dep:kael_bluetooth"]
maps = ["dep:kael_maps"]
ml = ["dep:kael_ml"]
nlp = ["dep:kael_nlp"]
licensing = ["dep:kael_licensing"]
updater = ["dep:kael_updater"]
sync = ["dep:kael_sync"]

# Convenience bundles
platform-full = ["storage", "icons", "diagnostics", "audio", "pdf", "document",
                 "notifications-full", "share", "search", "automation",
                 "location", "bluetooth", "maps", "ml", "nlp",
                 "licensing", "updater", "sync"]
```

---

## Workspace Registration

Add to root `Cargo.toml`:

```toml
[workspace]
members = [
    # ... existing members ...
    "crates/kael_storage",
    "crates/kael_icons",
    "crates/kael_diagnostics",
    "crates/kael_audio",
    "crates/kael_pdf",
    "crates/kael_document",
    "crates/kael_notifications",
    "crates/kael_share",
    "crates/kael_search",
    "crates/kael_automation",
    "crates/kael_location",
    "crates/kael_bluetooth",
    "crates/kael_maps",
    "crates/kael_ml",
    "crates/kael_nlp",
    "crates/kael_licensing",
    "crates/kael_updater",
    "crates/kael_sync",
]
```

---

## Summary

| Layer | Crate | Primary Dependency | LOC Estimate |
|-------|-------|--------------------|-------------|
| 0 | kael_storage | rusqlite | ~2,000 |
| 0 | kael_icons | resvg (existing) | ~1,500 + assets |
| 0 | kael_diagnostics | backtrace, minidump | ~3,000 |
| 1 | kael_audio | rodio, cpal, symphonia | ~4,000 |
| 1 | kael_pdf | mupdf (C) | ~5,000 |
| 1 | kael_document | kael_storage | ~3,000 |
| 2 | kael_notifications | platform FFI | ~2,500 |
| 2 | kael_share | platform FFI | ~1,500 |
| 2 | kael_search | platform FFI | ~2,000 |
| 2 | kael_automation | platform FFI | ~3,500 |
| 3 | kael_location | platform FFI | ~2,500 |
| 3 | kael_bluetooth | btleplug | ~3,000 |
| 3 | kael_maps | MapKit FFI + tile renderer | ~6,000 |
| 4 | kael_ml | onnxruntime (C) | ~4,000 |
| 4 | kael_nlp | NaturalLanguage FFI + whatlang | ~2,500 |
| 5 | kael_licensing | ring (crypto) | ~3,000 |
| 5 | kael_updater | ring, http_client | ~3,500 |
| 5 | kael_sync | kael_storage, platform FFI | ~4,000 |
| **Total** | | | **~57,000** |

Estimated implementation time: 6-8 weeks of focused development working layer by layer.

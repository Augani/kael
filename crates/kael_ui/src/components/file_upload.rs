use crate::astryx;
use crate::components::field::{Field, FieldStatusType};
use crate::components::field_status::FieldStatusVariant;
use crate::components::icon::Icon;
use crate::components::spinner::{Spinner, SpinnerSize, SpinnerVariant};
use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct SelectedFile {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub mime_type: Option<String>,
    pub is_image: bool,
}

impl SelectedFile {
    pub fn new(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        let is_image = matches!(
            extension.as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "ico"
        );

        let mime_type = match extension.as_str() {
            "png" => Some("image/png".to_string()),
            "jpg" | "jpeg" => Some("image/jpeg".to_string()),
            "gif" => Some("image/gif".to_string()),
            "bmp" => Some("image/bmp".to_string()),
            "webp" => Some("image/webp".to_string()),
            "svg" => Some("image/svg+xml".to_string()),
            "pdf" => Some("application/pdf".to_string()),
            "txt" => Some("text/plain".to_string()),
            "json" => Some("application/json".to_string()),
            "zip" => Some("application/zip".to_string()),
            _ => None,
        };

        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

        Self {
            name,
            path,
            size,
            mime_type,
            is_image,
        }
    }

    pub fn formatted_size(&self) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;

        if self.size >= GB {
            format!("{:.1} GB", self.size as f64 / GB as f64)
        } else if self.size >= MB {
            format!("{:.1} MB", self.size as f64 / MB as f64)
        } else if self.size >= KB {
            format!("{:.1} KB", self.size as f64 / KB as f64)
        } else {
            format!("{} B", self.size)
        }
    }
}

#[derive(Clone, Debug)]
pub struct FileUploadError {
    pub file_name: String,
    pub message: String,
}

pub struct FileUploadState {
    pub files: Vec<SelectedFile>,
    pub errors: Vec<FileUploadError>,
    pub is_dragging: bool,
    pub is_uploading: bool,
}

impl FileUploadState {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            errors: Vec::new(),
            is_dragging: false,
            is_uploading: false,
        }
    }

    pub fn add_file(&mut self, file: SelectedFile) {
        self.files.push(file);
    }

    pub fn remove_file(&mut self, index: usize) {
        if index < self.files.len() {
            self.files.remove(index);
        }
    }

    pub fn clear_files(&mut self) {
        self.files.clear();
    }

    pub fn add_error(&mut self, error: FileUploadError) {
        self.errors.push(error);
    }

    pub fn clear_errors(&mut self) {
        self.errors.clear();
    }

    pub fn set_dragging(&mut self, dragging: bool) {
        self.is_dragging = dragging;
    }

    pub fn set_uploading(&mut self, uploading: bool) {
        self.is_uploading = uploading;
    }

    pub fn has_files(&self) -> bool {
        !self.files.is_empty()
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }
}

impl Default for FileUploadState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum FileUploadSize {
    Sm,
    Md,
    Lg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FileUploadMode {
    Input,
    #[default]
    Dropzone,
}

impl FileUploadSize {
    fn min_height(&self) -> Pixels {
        match self {
            FileUploadSize::Sm => px(96.0),
            FileUploadSize::Md => px(128.0),
            FileUploadSize::Lg => px(160.0),
        }
    }

    fn icon_size(&self) -> Pixels {
        match self {
            FileUploadSize::Sm => px(20.0),
            FileUploadSize::Md => px(24.0),
            FileUploadSize::Lg => px(28.0),
        }
    }

    fn text_size(&self) -> Pixels {
        match self {
            FileUploadSize::Sm => px(13.0),
            FileUploadSize::Md => px(14.0),
            FileUploadSize::Lg => px(16.0),
        }
    }

    fn input_height(&self) -> Pixels {
        match self {
            FileUploadSize::Sm => px(28.0),
            FileUploadSize::Md => px(32.0),
            FileUploadSize::Lg => px(36.0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct FileTypeFilter {
    pub extensions: Vec<String>,
    pub label: String,
}

impl FileTypeFilter {
    pub fn new(extensions: Vec<&str>, label: impl Into<String>) -> Self {
        Self {
            extensions: extensions.into_iter().map(|s| s.to_string()).collect(),
            label: label.into(),
        }
    }

    pub fn images() -> Self {
        Self::new(
            vec!["png", "jpg", "jpeg", "gif", "bmp", "webp", "svg"],
            "Images",
        )
    }

    pub fn documents() -> Self {
        Self::new(vec!["pdf", "doc", "docx", "txt", "rtf"], "Documents")
    }

    pub fn all() -> Self {
        Self::new(vec![], "All files")
    }

    fn matches(&self, path: &std::path::Path) -> bool {
        if self.extensions.is_empty() {
            return true;
        }

        path.extension()
            .and_then(|e| e.to_str())
            .map(|ext| {
                self.extensions
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(ext))
            })
            .unwrap_or(false)
    }
}

#[derive(IntoElement)]
pub struct FileUpload {
    _id: ElementId,
    base: Stateful<Div>,
    state: Entity<FileUploadState>,
    label: Option<SharedString>,
    hidden_label: bool,
    description: Option<SharedString>,
    optional: bool,
    required: bool,
    size: FileUploadSize,
    mode: FileUploadMode,
    multiple: bool,
    max_file_size: Option<u64>,
    file_types: Option<FileTypeFilter>,
    disabled: bool,
    loading: bool,
    status: Option<FieldStatusType>,
    status_message: Option<SharedString>,
    show_file_list: bool,
    show_previews: bool,
    placeholder_text: Option<SharedString>,
    placeholder_icon: Option<SharedString>,
    on_files_changed: Option<Rc<dyn Fn(&[SelectedFile], &mut Window, &mut App)>>,
    on_error: Option<Rc<dyn Fn(&FileUploadError, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl FileUpload {
    pub fn new(id: impl Into<ElementId>, state: Entity<FileUploadState>) -> Self {
        let id = id.into();
        Self {
            _id: id.clone(),
            base: div().id(id),
            state,
            label: None,
            hidden_label: false,
            description: None,
            optional: false,
            required: false,
            size: FileUploadSize::Md,
            mode: FileUploadMode::default(),
            multiple: false,
            max_file_size: None,
            file_types: None,
            disabled: false,
            loading: false,
            status: None,
            status_message: None,
            show_file_list: true,
            show_previews: true,
            placeholder_text: None,
            placeholder_icon: None,
            on_files_changed: None,
            on_error: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn is_label_hidden(mut self, hidden: bool) -> Self {
        self.hidden_label = hidden;
        self
    }

    #[allow(non_snake_case)]
    pub fn isLabelHidden(self, hidden: bool) -> Self {
        self.is_label_hidden(hidden)
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn optional(mut self, optional: bool) -> Self {
        self.optional = optional;
        self
    }

    #[allow(non_snake_case)]
    pub fn isOptional(self, optional: bool) -> Self {
        self.optional(optional)
    }

    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    #[allow(non_snake_case)]
    pub fn isRequired(self, required: bool) -> Self {
        self.required(required)
    }

    pub fn size(mut self, size: FileUploadSize) -> Self {
        self.size = size;
        self
    }

    pub fn mode(mut self, mode: FileUploadMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn multiple(mut self, multiple: bool) -> Self {
        self.multiple = multiple;
        self
    }

    #[allow(non_snake_case)]
    pub fn isMultiple(self, multiple: bool) -> Self {
        self.multiple(multiple)
    }

    pub fn max_file_size(mut self, bytes: u64) -> Self {
        self.max_file_size = Some(bytes);
        self
    }

    #[allow(non_snake_case)]
    pub fn maxSize(self, bytes: u64) -> Self {
        self.max_file_size(bytes)
    }

    pub fn max_file_size_mb(mut self, mb: u64) -> Self {
        self.max_file_size = Some(mb * 1024 * 1024);
        self
    }

    pub fn file_types(mut self, filter: FileTypeFilter) -> Self {
        self.file_types = Some(filter);
        self
    }

    pub fn accept_images(self) -> Self {
        self.file_types(FileTypeFilter::images())
    }

    pub fn accept_documents(self) -> Self {
        self.file_types(FileTypeFilter::documents())
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[allow(non_snake_case)]
    pub fn isDisabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }

    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn is_loading(self, loading: bool) -> Self {
        self.loading(loading)
    }

    #[allow(non_snake_case)]
    pub fn isLoading(self, loading: bool) -> Self {
        self.loading(loading)
    }

    pub fn status(mut self, status: FieldStatusType, message: impl Into<SharedString>) -> Self {
        self.status = Some(status);
        self.status_message = Some(message.into());
        self
    }

    pub fn show_file_list(mut self, show: bool) -> Self {
        self.show_file_list = show;
        self
    }

    pub fn show_previews(mut self, show: bool) -> Self {
        self.show_previews = show;
        self
    }

    pub fn placeholder(mut self, text: impl Into<SharedString>) -> Self {
        self.placeholder_text = Some(text.into());
        self
    }

    pub fn placeholder_icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.placeholder_icon = Some(icon.into());
        self
    }

    pub fn on_files_changed<F>(mut self, handler: F) -> Self
    where
        F: Fn(&[SelectedFile], &mut Window, &mut App) + 'static,
    {
        self.on_files_changed = Some(Rc::new(handler));
        self
    }

    pub fn on_error<F>(mut self, handler: F) -> Self
    where
        F: Fn(&FileUploadError, &mut Window, &mut App) + 'static,
    {
        self.on_error = Some(Rc::new(handler));
        self
    }

    fn _validate_file(&self, path: &PathBuf) -> Result<(), String> {
        if let Some(ref filter) = self.file_types {
            if !filter.matches(path) {
                let allowed = if filter.extensions.is_empty() {
                    "all files".to_string()
                } else {
                    filter.extensions.join(", ")
                };
                return Err(format!("File type not allowed. Accepted: {}", allowed));
            }
        }

        if let Some(max_size) = self.max_file_size {
            let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            if file_size > max_size {
                let max_mb = max_size as f64 / (1024.0 * 1024.0);
                return Err(format!("File exceeds maximum size of {:.1} MB", max_mb));
            }
        }

        Ok(())
    }
}

impl Styled for FileUpload {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for FileUpload {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let state = self.state.read(cx);
        let is_dragging = state.is_dragging;
        let is_uploading = state.is_uploading || self.loading;
        let has_files = state.has_files();
        let files = state.files.clone();
        let errors = state.errors.clone();
        let user_style = self.style;
        let mode = self.mode;

        let disabled = self.disabled || is_uploading;
        let status_color = self.status.map(|status| match status {
            FieldStatusType::Warning => theme.tokens.warning,
            FieldStatusType::Error => theme.tokens.destructive,
            FieldStatusType::Success => theme.tokens.success,
        });

        let border_color = if disabled {
            theme.tokens.input
        } else if let Some(color) = status_color {
            color
        } else if is_dragging {
            theme.tokens.primary
        } else {
            theme.tokens.input
        };

        let bg_color = if disabled {
            theme.tokens.card
        } else if is_dragging {
            astryx::status_muted(theme.tokens.primary)
        } else {
            theme.tokens.card
        };

        let placeholder_text = self
            .placeholder_text
            .unwrap_or_else(|| "Drop files here or click to browse".into());

        let placeholder_icon = self
            .placeholder_icon
            .unwrap_or_else(|| "cloud-upload".into());

        let state_entity = self.state.clone();
        let multiple = self.multiple;
        let max_file_size = self.max_file_size;
        let file_types = self.file_types.clone();
        let on_files_changed = self.on_files_changed.clone();
        let _on_error = self.on_error.clone();

        let show_file_list = self.show_file_list;
        let show_previews = self.show_previews;
        let size = self.size.clone();
        let hover_ring = crate::astryx::input_hover_ring(theme.tokens.primary);
        let focus_ring = crate::astryx::focus_ring(theme.tokens.primary);

        let control = self
            .base
            .flex()
            .flex_col()
            .gap(px(12.0))
            .w_full()
            .map(|mut this| {
                this.style().refine(&user_style);
                this
            })
            .child(
                div()
                    .id("drop-zone")
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(8.0))
                    .w_full()
                    .when(mode == FileUploadMode::Dropzone, |this| {
                        this.flex_col()
                            .min_h(size.min_height())
                            .px(px(16.0))
                            .py(px(24.0))
                            .border_dashed()
                    })
                    .when(mode == FileUploadMode::Input, |this| {
                        this.flex_row()
                            .h(size.input_height())
                            .px(px(12.0))
                            .py(px(0.0))
                    })
                    .rounded(theme.tokens.radius_md)
                    .border_1()
                    .border_color(border_color)
                    .bg(bg_color)
                    .transition(theme.tokens.transition_fast)
                    .when(is_dragging && !disabled, |this| {
                        this.shadow(smallvec::smallvec![focus_ring])
                    })
                    .when(!disabled, |this| {
                        this.cursor(CursorStyle::PointingHand).hover(move |style| {
                            style
                                .border_color(theme.tokens.primary)
                                .shadow(smallvec::smallvec![hover_ring])
                        })
                    })
                    .when(disabled, |this| {
                        this.cursor(CursorStyle::Arrow).opacity(0.5)
                    })
                    .when(is_uploading, |this| {
                        this.child(
                            div()
                                .flex()
                                .when(mode == FileUploadMode::Dropzone, |this| this.flex_col())
                                .when(mode == FileUploadMode::Input, |this| this.flex_row())
                                .items_center()
                                .gap(px(12.0))
                                .child(
                                    Spinner::new()
                                        .size(if mode == FileUploadMode::Input {
                                            SpinnerSize::Sm
                                        } else {
                                            SpinnerSize::Lg
                                        })
                                        .variant(SpinnerVariant::Primary),
                                )
                                .child(
                                    div()
                                        .text_size(size.text_size())
                                        .text_color(theme.tokens.muted_foreground)
                                        .child("Uploading..."),
                                ),
                        )
                    })
                    .when(!is_uploading, |this| {
                        this.child(
                            div()
                                .flex()
                                .when(mode == FileUploadMode::Dropzone, |this| this.flex_col())
                                .when(mode == FileUploadMode::Input, |this| this.flex_row())
                                .items_center()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .size(size.icon_size())
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            Icon::new(placeholder_icon.to_string())
                                                .size(size.icon_size())
                                                .color(if is_dragging {
                                                    theme.tokens.primary
                                                } else {
                                                    theme.tokens.muted_foreground
                                                }),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .text_size(size.text_size())
                                        .text_color(if is_dragging {
                                            theme.tokens.primary
                                        } else {
                                            theme.tokens.foreground
                                        })
                                        .font_weight(FontWeight::MEDIUM)
                                        .when(mode == FileUploadMode::Input, |this| {
                                            this.overflow_hidden()
                                                .text_ellipsis()
                                                .whitespace_nowrap()
                                        })
                                        .child(placeholder_text.clone()),
                                )
                                .when(mode == FileUploadMode::Dropzone, |this| {
                                    this.when_some(file_types.clone(), |this, filter| {
                                        this.child(
                                            div()
                                                .text_size(px(12.0))
                                                .text_color(theme.tokens.muted_foreground)
                                                .child(if filter.extensions.is_empty() {
                                                    "All file types supported".to_string()
                                                } else {
                                                    format!(
                                                        "Accepted: {}",
                                                        filter.extensions.join(", ")
                                                    )
                                                }),
                                        )
                                    })
                                })
                                .when(mode == FileUploadMode::Dropzone, |this| {
                                    this.when_some(max_file_size, |this, max_size| {
                                        let max_mb = max_size as f64 / (1024.0 * 1024.0);
                                        this.child(
                                            div()
                                                .text_size(px(12.0))
                                                .text_color(theme.tokens.muted_foreground)
                                                .child(format!("Max size: {:.0} MB", max_mb)),
                                        )
                                    })
                                }),
                        )
                    })
                    .on_click({
                        let state_entity = state_entity.clone();
                        let file_types = file_types.clone();
                        move |_, window, cx| {
                            if disabled {
                                return;
                            }

                            let receiver = cx.prompt_for_paths(PathPromptOptions {
                                files: true,
                                directories: false,
                                multiple,
                                prompt: Some("Select files".into()),
                            });

                            let state_entity = state_entity.clone();
                            let file_types = file_types.clone();

                            window
                                .spawn(cx, async move |cx| {
                                    if let Ok(Ok(Some(paths))) = receiver.await {
                                        for path in paths {
                                            let file_name = path
                                                .file_name()
                                                .map(|n| n.to_string_lossy().to_string())
                                                .unwrap_or_default();

                                            if let Some(ref filter) = file_types {
                                                if !filter.matches(&path) {
                                                    let error = FileUploadError {
                                                        file_name: file_name.clone(),
                                                        message: format!(
                                                            "File type not allowed. Accepted: {}",
                                                            filter.extensions.join(", ")
                                                        ),
                                                    };
                                                    let state_entity = state_entity.clone();
                                                    cx.update(|_, cx| {
                                                        state_entity.update(cx, |state, _| {
                                                            state.add_error(error);
                                                        });
                                                    })
                                                    .ok();
                                                    continue;
                                                }
                                            }

                                            if let Some(max_size) = max_file_size {
                                                let file_size = std::fs::metadata(&path)
                                                    .map(|m| m.len())
                                                    .unwrap_or(0);
                                                if file_size > max_size {
                                                    let max_mb =
                                                        max_size as f64 / (1024.0 * 1024.0);
                                                    let error = FileUploadError {
                                                        file_name: file_name.clone(),
                                                        message: format!(
                                                            "File exceeds maximum size of {:.1} MB",
                                                            max_mb
                                                        ),
                                                    };
                                                    let state_entity = state_entity.clone();
                                                    cx.update(|_, cx| {
                                                        state_entity.update(cx, |state, _| {
                                                            state.add_error(error);
                                                        });
                                                    })
                                                    .ok();
                                                    continue;
                                                }
                                            }

                                            let selected_file = SelectedFile::new(path);
                                            let state_entity = state_entity.clone();
                                            cx.update(|_, cx| {
                                                state_entity.update(cx, |state, _| {
                                                    state.add_file(selected_file);
                                                });
                                            })
                                            .ok();
                                        }
                                    }
                                })
                                .detach();
                        }
                    }),
            )
            .when(!errors.is_empty(), |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .children(errors.iter().map(|error| {
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .px(px(12.0))
                                .py(px(8.0))
                                .rounded(theme.tokens.radius_md)
                                .bg(theme.tokens.destructive.opacity(0.1))
                                .border_1()
                                .border_color(theme.tokens.destructive.opacity(0.3))
                                .child(
                                    Icon::new("circle-alert")
                                        .size(px(16.0))
                                        .color(theme.tokens.destructive),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.0))
                                        .child(
                                            div()
                                                .text_size(px(13.0))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(theme.tokens.destructive)
                                                .child(error.file_name.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.0))
                                                .text_color(theme.tokens.destructive.opacity(0.8))
                                                .child(error.message.clone()),
                                        ),
                                )
                        })),
                )
            })
            .when(
                show_file_list && has_files && mode == FileUploadMode::Dropzone,
                |this| {
                    let state_entity = state_entity.clone();
                    let on_files_changed = on_files_changed.clone();

                    this.child(div().flex().flex_col().gap(px(8.0)).children(
                        files.iter().enumerate().map(|(index, file)| {
                            let state_entity = state_entity.clone();
                            let on_files_changed = on_files_changed.clone();
                            let file_name = file.name.clone();
                            let file_size = file.formatted_size();
                            let is_image = file.is_image;
                            let file_path = file.path.clone();

                            div()
                                .id(("file-item", index))
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .px(px(8.0))
                                .py(px(6.0))
                                .rounded(theme.tokens.radius_md)
                                .bg(theme.tokens.card)
                                .border_1()
                                .border_color(theme.tokens.input)
                                .when(show_previews && is_image, |this| {
                                    this.child(
                                        div()
                                            .w(px(36.0))
                                            .h(px(36.0))
                                            .rounded(theme.tokens.radius_sm)
                                            .bg(theme.tokens.muted)
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .overflow_hidden()
                                            .child(
                                                img(file_path.to_string_lossy().to_string())
                                                    .w(px(36.0))
                                                    .h(px(36.0))
                                                    .object_fit(ObjectFit::Cover),
                                            ),
                                    )
                                })
                                .when(!show_previews || !is_image, |this| {
                                    this.child(
                                        div()
                                            .w(px(32.0))
                                            .h(px(32.0))
                                            .rounded(theme.tokens.radius_sm)
                                            .bg(theme.tokens.muted)
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(
                                                Icon::new(if is_image { "image" } else { "file" })
                                                    .size(px(16.0))
                                                    .color(theme.tokens.muted_foreground),
                                            ),
                                    )
                                })
                                .child(
                                    div()
                                        .flex_1()
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.0))
                                        .overflow_hidden()
                                        .child(
                                            div()
                                                .text_size(px(13.0))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(theme.tokens.foreground)
                                                .overflow_hidden()
                                                .text_ellipsis()
                                                .whitespace_nowrap()
                                                .child(file_name),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.0))
                                                .text_color(theme.tokens.muted_foreground)
                                                .child(file_size),
                                        ),
                                )
                                .when(!disabled, |this| {
                                    this.child(
                                        div()
                                            .id(("remove-btn", index))
                                            .w(px(20.0))
                                            .h(px(20.0))
                                            .rounded_full()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor(CursorStyle::PointingHand)
                                            .transition(theme.tokens.transition_fast)
                                            .hover(|style| {
                                                style.bg(theme.tokens.destructive.opacity(0.1))
                                            })
                                            .child(
                                                Icon::new("x")
                                                    .size(px(14.0))
                                                    .color(theme.tokens.muted_foreground),
                                            )
                                            .on_click(move |_, window, cx| {
                                                state_entity.update(cx, |state, _| {
                                                    state.remove_file(index);
                                                });
                                                if let Some(ref handler) = on_files_changed {
                                                    let files = state_entity.read(cx).files.clone();
                                                    handler(&files, window, cx);
                                                }
                                            }),
                                    )
                                })
                        }),
                    ))
                },
            );

        match self.label {
            Some(label) => {
                let mut field = Field::new(label, control)
                    .hidden_label(self.hidden_label)
                    .optional(self.optional)
                    .required(self.required)
                    .disabled(self.disabled)
                    .status_variant(FieldStatusVariant::Detached);

                if let Some(description) = self.description {
                    field = field.description(description);
                }

                if let Some((status, message)) = self.status.zip(self.status_message) {
                    field = field.status(status, message);
                }

                field.into_any_element()
            }
            None => control.into_any_element(),
        }
    }
}

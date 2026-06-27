//! Table - Simple table component for structured data display.

use crate::theme::use_theme;
use kael::{prelude::FluentBuilder as _, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TableDensity {
    Compact,
    #[default]
    Balanced,
    Spacious,
}

impl TableDensity {
    fn vertical_padding(self) -> Pixels {
        match self {
            Self::Compact => px(4.0),
            Self::Balanced => px(8.0),
            Self::Spacious => px(12.0),
        }
    }

    fn horizontal_padding(self) -> Pixels {
        match self {
            Self::Compact => px(8.0),
            Self::Balanced => px(12.0),
            Self::Spacious => px(16.0),
        }
    }

    fn text_size(self) -> Pixels {
        px(14.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TableDividers {
    #[default]
    Rows,
    Columns,
    Grid,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TableTextOverflow {
    #[default]
    Wrap,
    Truncate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TableColumnAlign {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TableVerticalAlign {
    Top,
    #[default]
    Middle,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColumnWidth {
    Proportional {
        value: f32,
        min_width: Option<Pixels>,
    },
    Pixel(Pixels),
}

pub type ProportionalWidth = ColumnWidth;
pub type PixelWidth = ColumnWidth;

pub const DEFAULT_MIN_COLUMN_WIDTH: Pixels = px(120.0);

pub fn proportional(value: f32) -> ColumnWidth {
    ColumnWidth::Proportional {
        value,
        min_width: Some(DEFAULT_MIN_COLUMN_WIDTH),
    }
}

pub fn pixel(value: impl Into<Pixels>) -> ColumnWidth {
    ColumnWidth::Pixel(value.into())
}

impl ColumnWidth {
    fn resolve(self, fallback: Pixels) -> Pixels {
        match self {
            Self::Pixel(width) => width,
            Self::Proportional { value, min_width } => {
                min_width.unwrap_or_else(|| px((120.0 * value.max(0.25)).max(1.0)))
            }
        }
        .max(fallback)
    }
}

#[derive(Clone)]
pub struct TableColumn {
    pub header: SharedString,
    pub width: Option<Pixels>,
    pub column_width: Option<ColumnWidth>,
    pub align: TableColumnAlign,
    pub resizable: bool,
    pub sortable: bool,
}

impl TableColumn {
    pub fn new<T: Into<SharedString>>(header: T) -> Self {
        Self {
            header: header.into(),
            width: None,
            column_width: None,
            align: TableColumnAlign::Start,
            resizable: true,
            sortable: false,
        }
    }

    pub fn width(mut self, width: impl Into<Pixels>) -> Self {
        self.width = Some(width.into());
        self
    }

    pub fn column_width(mut self, width: ColumnWidth) -> Self {
        self.column_width = Some(width);
        self
    }

    #[allow(non_snake_case)]
    pub fn columnWidth(self, width: ColumnWidth) -> Self {
        self.column_width(width)
    }

    pub fn align(mut self, align: TableColumnAlign) -> Self {
        self.align = align;
        self
    }

    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    pub fn sortable(mut self, sortable: bool) -> Self {
        self.sortable = sortable;
        self
    }

    fn resolved_width(&self) -> Pixels {
        self.column_width
            .map(|width| width.resolve(px(0.0)))
            .or(self.width)
            .unwrap_or(DEFAULT_MIN_COLUMN_WIDTH)
    }
}

pub struct TableRow {
    pub cells: Vec<SharedString>,
    pub selected: bool,
    children: Vec<AnyElement>,
    is_header: bool,
    style: StyleRefinement,
}

impl TableRow {
    pub fn new(cells: Vec<SharedString>) -> Self {
        Self {
            cells,
            selected: false,
            children: Vec::new(),
            is_header: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn children() -> Self {
        Self {
            cells: Vec::new(),
            selected: false,
            children: Vec::new(),
            is_header: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn header(mut self, is_header: bool) -> Self {
        self.is_header = is_header;
        self
    }

    pub fn cell(mut self, cell: impl IntoElement) -> Self {
        self.children.push(cell.into_any_element());
        self
    }
}

impl Styled for TableRow {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl IntoElement for TableRow {
    type Element = AnyElement;

    fn into_element(self) -> Self::Element {
        let theme = use_theme();
        let user_style = self.style;
        let cells = self.cells;
        let children = self.children;
        let is_header = self.is_header;

        div()
            .flex()
            .transition(theme.tokens.transition_fast)
            .bg(if self.selected {
                theme.tokens.accent.opacity(0.36)
            } else {
                kael::transparent_black()
            })
            .when(self.selected, |this| {
                this.border_l_2().border_color(theme.tokens.primary)
            })
            .children(children)
            .children(cells.into_iter().map(move |cell| {
                if is_header {
                    TableHeaderCell::new(cell).into_any_element()
                } else {
                    TableCell::new(cell).into_any_element()
                }
            }))
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .into_any_element()
    }
}

#[derive(IntoElement)]
pub struct TableHeader {
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl TableHeader {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }
}

impl Default for TableHeader {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for TableHeader {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TableHeader {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let user_style = self.style;
        div().flex().flex_col().children(self.children).map(|this| {
            let mut div = this;
            div.style().refine(&user_style);
            div
        })
    }
}

#[derive(IntoElement)]
pub struct TableBody {
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl TableBody {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }
}

impl Default for TableBody {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for TableBody {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TableBody {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let user_style = self.style;
        div().flex().flex_col().children(self.children).map(|this| {
            let mut div = this;
            div.style().refine(&user_style);
            div
        })
    }
}

#[derive(IntoElement)]
pub struct TableFooter {
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl TableFooter {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }
}

impl Default for TableFooter {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for TableFooter {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TableFooter {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let theme = use_theme();
        let user_style = self.style;
        div()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(theme.tokens.border)
            .children(self.children)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(IntoElement)]
pub struct TableCell {
    content: AnyElement,
    width: Option<Pixels>,
    align: TableColumnAlign,
    vertical_align: TableVerticalAlign,
    col_span: usize,
    row_span: usize,
    text_overflow: TableTextOverflow,
    style: StyleRefinement,
}

impl TableCell {
    pub fn new(content: impl IntoElement) -> Self {
        Self {
            content: content.into_any_element(),
            width: None,
            align: TableColumnAlign::Start,
            vertical_align: TableVerticalAlign::Middle,
            col_span: 1,
            row_span: 1,
            text_overflow: TableTextOverflow::Wrap,
            style: StyleRefinement::default(),
        }
    }

    pub fn width(mut self, width: impl Into<Pixels>) -> Self {
        self.width = Some(width.into());
        self
    }

    pub fn align(mut self, align: TableColumnAlign) -> Self {
        self.align = align;
        self
    }

    pub fn vertical_align(mut self, vertical_align: TableVerticalAlign) -> Self {
        self.vertical_align = vertical_align;
        self
    }

    pub fn col_span(mut self, col_span: usize) -> Self {
        self.col_span = col_span.max(1);
        self
    }

    #[allow(non_snake_case)]
    pub fn colSpan(self, col_span: usize) -> Self {
        self.col_span(col_span)
    }

    pub fn row_span(mut self, row_span: usize) -> Self {
        self.row_span = row_span.max(1);
        self
    }

    #[allow(non_snake_case)]
    pub fn rowSpan(self, row_span: usize) -> Self {
        self.row_span(row_span)
    }

    pub fn text_overflow(mut self, text_overflow: TableTextOverflow) -> Self {
        self.text_overflow = text_overflow;
        self
    }
}

impl Styled for TableCell {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TableCell {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let theme = use_theme();
        let user_style = self.style;
        let col_span = self.col_span as f32;
        let align = self.align;
        let vertical_align = self.vertical_align;

        div()
            .flex()
            .min_w(px(0.0))
            .when_some(self.width, |this, width| this.w(width * col_span))
            .when(self.width.is_none(), |this| this.flex_1())
            .when(align == TableColumnAlign::Start, |this| {
                this.justify_start()
            })
            .when(align == TableColumnAlign::Center, |this| {
                this.justify_center()
            })
            .when(align == TableColumnAlign::End, |this| this.justify_end())
            .when(vertical_align == TableVerticalAlign::Top, |this| {
                this.items_start()
            })
            .when(vertical_align == TableVerticalAlign::Middle, |this| {
                this.items_center()
            })
            .when(vertical_align == TableVerticalAlign::Bottom, |this| {
                this.items_end()
            })
            .px(px(12.0))
            .py(px(8.0))
            .text_size(px(14.0))
            .text_color(theme.tokens.foreground)
            .border_b_1()
            .border_color(theme.tokens.border)
            .when(self.text_overflow == TableTextOverflow::Truncate, |this| {
                this.overflow_hidden().text_ellipsis().whitespace_nowrap()
            })
            .child(self.content)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(IntoElement)]
pub struct TableHeaderCell {
    content: AnyElement,
    width: Option<Pixels>,
    align: TableColumnAlign,
    scope: SharedString,
    style: StyleRefinement,
}

impl TableHeaderCell {
    pub fn new(content: impl IntoElement) -> Self {
        Self {
            content: content.into_any_element(),
            width: None,
            align: TableColumnAlign::Start,
            scope: "col".into(),
            style: StyleRefinement::default(),
        }
    }

    pub fn width(mut self, width: impl Into<Pixels>) -> Self {
        self.width = Some(width.into());
        self
    }

    pub fn align(mut self, align: TableColumnAlign) -> Self {
        self.align = align;
        self
    }

    pub fn scope(mut self, scope: impl Into<SharedString>) -> Self {
        self.scope = scope.into();
        self
    }
}

impl Styled for TableHeaderCell {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TableHeaderCell {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let theme = use_theme();
        let user_style = self.style;
        let align = self.align;
        let _scope = self.scope;

        div()
            .flex()
            .items_center()
            .min_w(px(0.0))
            .when_some(self.width, |this, width| this.w(width))
            .when(self.width.is_none(), |this| this.flex_1())
            .when(align == TableColumnAlign::Start, |this| {
                this.justify_start()
            })
            .when(align == TableColumnAlign::Center, |this| {
                this.justify_center()
            })
            .when(align == TableColumnAlign::End, |this| this.justify_end())
            .px(px(12.0))
            .py(px(8.0))
            .text_size(px(13.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.tokens.muted_foreground)
            .border_b_1()
            .border_color(theme.tokens.border)
            .overflow_hidden()
            .text_ellipsis()
            .whitespace_nowrap()
            .child(self.content)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

pub struct Table {
    columns: Vec<TableColumn>,
    rows: Vec<TableRow>,
    children: Vec<AnyElement>,
    density: TableDensity,
    dividers: TableDividers,
    striped: bool,
    hover: bool,
    vertical_align: TableVerticalAlign,
    text_overflow: TableTextOverflow,
    style: StyleRefinement,
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

impl Table {
    pub fn new() -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            children: Vec::new(),
            density: TableDensity::Balanced,
            dividers: TableDividers::Rows,
            striped: false,
            hover: false,
            vertical_align: TableVerticalAlign::Middle,
            text_overflow: TableTextOverflow::Wrap,
            style: StyleRefinement::default(),
        }
    }

    pub fn columns(mut self, columns: Vec<TableColumn>) -> Self {
        self.columns = columns;
        self
    }

    pub fn rows(mut self, rows: Vec<TableRow>) -> Self {
        self.rows = rows;
        self
    }

    pub fn density(mut self, density: TableDensity) -> Self {
        self.density = density;
        self
    }

    pub fn dividers(mut self, dividers: TableDividers) -> Self {
        self.dividers = dividers;
        self
    }

    pub fn striped(mut self, striped: bool) -> Self {
        self.striped = striped;
        self
    }

    pub fn hover(mut self, hover: bool) -> Self {
        self.hover = hover;
        self
    }

    pub fn text_overflow(mut self, text_overflow: TableTextOverflow) -> Self {
        self.text_overflow = text_overflow;
        self
    }

    #[allow(non_snake_case)]
    pub fn textOverflow(self, text_overflow: TableTextOverflow) -> Self {
        self.text_overflow(text_overflow)
    }

    pub fn vertical_align(mut self, vertical_align: TableVerticalAlign) -> Self {
        self.vertical_align = vertical_align;
        self
    }

    #[allow(non_snake_case)]
    pub fn verticalAlign(self, vertical_align: TableVerticalAlign) -> Self {
        self.vertical_align(vertical_align)
    }

    pub fn has_hover(self, hover: bool) -> Self {
        self.hover(hover)
    }

    #[allow(non_snake_case)]
    pub fn hasHover(self, hover: bool) -> Self {
        self.has_hover(hover)
    }

    pub fn is_striped(self, striped: bool) -> Self {
        self.striped(striped)
    }

    #[allow(non_snake_case)]
    pub fn isStriped(self, striped: bool) -> Self {
        self.is_striped(striped)
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.children
            .extend(children.into_iter().map(|child| child.into_any_element()));
        self
    }
}

impl Styled for Table {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl IntoElement for Table {
    type Element = AnyElement;

    fn into_element(self) -> Self::Element {
        let theme = use_theme();
        let user_style = self.style;
        let density = self.density;
        let dividers = self.dividers;
        let text_overflow = self.text_overflow;
        let vertical_align = self.vertical_align;
        let cell_py = density.vertical_padding();
        let cell_px = density.horizontal_padding();
        let text_size = density.text_size();
        let show_row_dividers = matches!(dividers, TableDividers::Rows | TableDividers::Grid);
        let show_column_dividers = matches!(dividers, TableDividers::Columns | TableDividers::Grid);

        if !self.children.is_empty() {
            return div()
                .id("table-scroll-wrapper")
                .overflow_x_scroll()
                .child(div().flex().flex_col().children(self.children))
                .map(|this| {
                    let mut div = this;
                    div.style().refine(&user_style);
                    div
                })
                .into_any_element();
        }

        let column_count = self.columns.len();
        let header_cells = self.columns.iter().enumerate().map(|(col_index, column)| {
            let width = column.resolved_width();
            div()
                .flex()
                .items_center()
                .when(column.align == TableColumnAlign::Start, |this| {
                    this.justify_start()
                })
                .when(column.align == TableColumnAlign::Center, |this| {
                    this.justify_center()
                })
                .when(column.align == TableColumnAlign::End, |this| {
                    this.justify_end()
                })
                .px(cell_px)
                .py(cell_py)
                .w(width)
                .text_size(text_size)
                .font_family(theme.tokens.font_family.clone())
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.tokens.muted_foreground)
                .overflow_hidden()
                .text_ellipsis()
                .border_b_1()
                .border_color(theme.tokens.border)
                .when(
                    show_column_dividers && col_index + 1 < column_count,
                    |this| this.border_r_1().border_color(theme.tokens.border),
                )
                .child(column.header.clone())
        });

        let header = div().flex().children(header_cells);

        let row_count = self.rows.len();
        let row_elements = self.rows.into_iter().enumerate().map(|(row_index, row)| {
            let row_selected = row.selected;
            let striped_bg = self.striped && row_index % 2 == 1;
            let is_last_row = row_index + 1 == row_count;
            let cell_elements = row.cells.iter().enumerate().map(|(col_index, cell)| {
                let Some(column) = self.columns.get(col_index) else {
                    return div().into_any_element();
                };
                let width = column.resolved_width();

                div()
                    .flex()
                    .items_center()
                    .when(vertical_align == TableVerticalAlign::Top, |this| {
                        this.items_start()
                    })
                    .when(vertical_align == TableVerticalAlign::Middle, |this| {
                        this.items_center()
                    })
                    .when(vertical_align == TableVerticalAlign::Bottom, |this| {
                        this.items_end()
                    })
                    .when(column.align == TableColumnAlign::Start, |this| {
                        this.justify_start()
                    })
                    .when(column.align == TableColumnAlign::Center, |this| {
                        this.justify_center()
                    })
                    .when(column.align == TableColumnAlign::End, |this| {
                        this.justify_end()
                    })
                    .px(cell_px)
                    .py(cell_py)
                    .w(width)
                    .text_size(text_size)
                    .font_family(theme.tokens.font_family.clone())
                    .text_color(theme.tokens.foreground)
                    .when(show_row_dividers && !is_last_row, |this| {
                        this.border_b_1().border_color(theme.tokens.border)
                    })
                    .when(
                        show_column_dividers && col_index + 1 < column_count,
                        |this| this.border_r_1().border_color(theme.tokens.border),
                    )
                    .when(text_overflow == TableTextOverflow::Truncate, |this| {
                        this.overflow_hidden().text_ellipsis().whitespace_nowrap()
                    })
                    .child(cell.clone())
                    .into_any_element()
            });

            div()
                .flex()
                .transition(theme.tokens.transition_fast)
                .bg(if row_selected {
                    theme.tokens.accent.opacity(0.36)
                } else if striped_bg {
                    theme.tokens.muted
                } else {
                    kael::transparent_black()
                })
                .when(self.hover, |this| {
                    this.hover(|style| {
                        style.bg(crate::astryx::overlay_hover(
                            theme.tokens.background.l < 0.5,
                        ))
                    })
                })
                .children(cell_elements)
        });

        div()
            .id("table-scroll-wrapper")
            .overflow_x_scroll()
            .child(div().flex().flex_col().child(header).children(row_elements))
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .into_any_element()
    }
}

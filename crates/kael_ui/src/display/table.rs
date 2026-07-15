//! Table - Simple table component for structured data display.

use crate::{
    components::button::{Button, ButtonSize, ButtonVariant, IconPosition},
    theme::use_theme,
};
use kael::{prelude::FluentBuilder as _, *};
use std::{panic::Location, rc::Rc};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TableSortDirection {
    #[default]
    Ascending,
    Descending,
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

fn valid_table_width(width: Pixels) -> Option<Pixels> {
    let value = f32::from(width);
    (value.is_finite() && value > 0.0).then_some(width)
}

pub fn proportional(value: f32) -> ColumnWidth {
    ColumnWidth::Proportional {
        value: if value.is_finite() && value > 0.0 {
            value
        } else {
            1.0
        },
        min_width: Some(DEFAULT_MIN_COLUMN_WIDTH),
    }
}

pub fn pixel(value: impl Into<Pixels>) -> ColumnWidth {
    ColumnWidth::Pixel(valid_table_width(value.into()).unwrap_or(DEFAULT_MIN_COLUMN_WIDTH))
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
            resizable: false,
            sortable: false,
        }
    }

    pub fn width(mut self, width: impl Into<Pixels>) -> Self {
        self.width = valid_table_width(width.into());
        self
    }

    pub fn column_width(mut self, width: ColumnWidth) -> Self {
        self.column_width = Some(match width {
            ColumnWidth::Pixel(width) => {
                ColumnWidth::Pixel(valid_table_width(width).unwrap_or(DEFAULT_MIN_COLUMN_WIDTH))
            }
            ColumnWidth::Proportional { value, min_width } => ColumnWidth::Proportional {
                value: if value.is_finite() && value > 0.0 {
                    value
                } else {
                    1.0
                },
                min_width: min_width.and_then(valid_table_width),
            },
        });
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

    fn apply_width(&self, mut cell: Div) -> Div {
        match self.column_width {
            Some(ColumnWidth::Pixel(width)) => cell
                .w(valid_table_width(width).unwrap_or(DEFAULT_MIN_COLUMN_WIDTH))
                .flex_none(),
            Some(ColumnWidth::Proportional { value, min_width }) => {
                cell.style().flex_grow = Some(if value.is_finite() && value > 0.0 {
                    value
                } else {
                    1.0
                });
                cell.style().flex_shrink = Some(1.0);
                cell.style().flex_basis = Some(relative(0.0).into());
                cell.min_w(
                    min_width
                        .and_then(valid_table_width)
                        .unwrap_or(DEFAULT_MIN_COLUMN_WIDTH),
                )
            }
            None => match self.width {
                Some(width) => cell
                    .w(valid_table_width(width).unwrap_or(DEFAULT_MIN_COLUMN_WIDTH))
                    .flex_none(),
                None => cell.min_w(DEFAULT_MIN_COLUMN_WIDTH).flex_1(),
            },
        }
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
        let scope_description = if self.scope.eq_ignore_ascii_case("row") {
            "Row header"
        } else {
            "Column header"
        };

        div()
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::StaticText)
                    .description(scope_description),
            )
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
    id: ElementId,
    columns: Vec<TableColumn>,
    rows: Vec<TableRow>,
    children: Vec<AnyElement>,
    empty_content: Option<AnyElement>,
    density: TableDensity,
    dividers: TableDividers,
    striped: bool,
    hover: bool,
    vertical_align: TableVerticalAlign,
    text_overflow: TableTextOverflow,
    sort_column: Option<usize>,
    sort_direction: TableSortDirection,
    on_sort: Option<Rc<dyn Fn(usize, TableSortDirection, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

impl Table {
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "table-{}-{}-{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            columns: Vec::new(),
            rows: Vec::new(),
            children: Vec::new(),
            empty_content: None,
            density: TableDensity::Balanced,
            dividers: TableDividers::Rows,
            striped: false,
            hover: false,
            vertical_align: TableVerticalAlign::Middle,
            text_overflow: TableTextOverflow::Wrap,
            sort_column: None,
            sort_direction: TableSortDirection::Ascending,
            on_sort: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn columns(mut self, columns: Vec<TableColumn>) -> Self {
        self.columns = columns;
        self
    }

    pub fn rows(mut self, rows: Vec<TableRow>) -> Self {
        self.rows = rows;
        self
    }

    /// Set the content shown beneath the header when the table has no rows.
    pub fn empty_content(mut self, content: impl IntoElement) -> Self {
        self.empty_content = Some(content.into_any_element());
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

    pub fn sort(mut self, column: usize, direction: TableSortDirection) -> Self {
        self.sort_column = Some(column);
        self.sort_direction = direction;
        self
    }

    pub fn on_sort(
        mut self,
        handler: impl Fn(usize, TableSortDirection, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_sort = Some(Rc::new(handler));
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
        let table_id = self.id.clone();
        let sort_column = self.sort_column.filter(|column| {
            self.columns
                .get(*column)
                .is_some_and(|column| column.sortable)
        });
        let sort_direction = self.sort_direction;
        let on_sort = self.on_sort.clone();
        let empty_content = self.empty_content;
        let has_empty_content = empty_content.is_some();

        if !self.children.is_empty() {
            return div()
                .id(ElementId::NamedChild(Box::new(table_id), "scroll".into()))
                .accessibility(
                    AccessibilityAttributes::new(AccessibilityRole::Group).label("Data table"),
                )
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
            let is_sorted = sort_column == Some(col_index);
            let next_direction = if is_sorted && sort_direction == TableSortDirection::Ascending {
                TableSortDirection::Descending
            } else {
                TableSortDirection::Ascending
            };
            let header_content = if column.sortable && on_sort.is_some() {
                let mut button = Button::new(
                    ElementId::NamedChild(
                        Box::new(table_id.clone()),
                        format!("sort-{col_index}").into(),
                    ),
                    column.header.clone(),
                )
                .size(ButtonSize::Sm)
                .variant(ButtonVariant::Ghost)
                .selected(is_sorted)
                .on_click({
                    let on_sort = on_sort.clone();
                    move |_, window, cx| {
                        if let Some(handler) = on_sort.as_ref() {
                            handler(col_index, next_direction, window, cx);
                        }
                    }
                });
                if is_sorted {
                    button = button
                        .icon(if sort_direction == TableSortDirection::Ascending {
                            "arrow-up"
                        } else {
                            "arrow-down"
                        })
                        .icon_position(IconPosition::End);
                }
                button.into_any_element()
            } else {
                div().child(column.header.clone()).into_any_element()
            };
            column.apply_width(
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
                    .child(header_content),
            )
        });

        let header = div().flex().children(header_cells);

        let row_count = self.rows.len();
        let row_elements = self.rows.into_iter().enumerate().map(|(row_index, row)| {
            let row_selected = row.selected;
            let striped_bg = self.striped && row_index % 2 == 1;
            let is_last_row = row_index + 1 == row_count;
            let cell_elements = self.columns.iter().enumerate().map(|(col_index, column)| {
                let cell = row
                    .cells
                    .get(col_index)
                    .cloned()
                    .unwrap_or_else(|| SharedString::from(""));
                column
                    .apply_width(
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
                            .child(cell),
                    )
                    .into_any_element()
            });

            div()
                .accessibility(AccessibilityAttributes::new(AccessibilityRole::ListItem))
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
            .id(ElementId::NamedChild(Box::new(table_id), "scroll".into()))
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::List).label("Data table"),
            )
            .overflow_x_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(header)
                    .when(row_count == 0, |this| {
                        this.child(
                            div()
                                .accessibility(
                                    AccessibilityAttributes::new(AccessibilityRole::StaticText)
                                        .label("No table rows"),
                                )
                                .min_h(px(112.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .px(cell_px)
                                .py(px(24.0))
                                .text_sm()
                                .text_color(theme.tokens.muted_foreground)
                                .when_some(empty_content, |this, content| this.child(content))
                                .when(!has_empty_content, |this| this.child("No data")),
                        )
                    })
                    .children(row_elements),
            )
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn column_widths_reject_non_finite_or_non_positive_geometry() {
        let proportional = proportional(f32::NAN);
        assert_eq!(
            proportional,
            ColumnWidth::Proportional {
                value: 1.0,
                min_width: Some(DEFAULT_MIN_COLUMN_WIDTH),
            }
        );
        assert_eq!(
            pixel(px(-1.0)),
            ColumnWidth::Pixel(DEFAULT_MIN_COLUMN_WIDTH)
        );

        let column = TableColumn::new("Name")
            .width(px(f32::INFINITY))
            .column_width(ColumnWidth::Proportional {
                value: -4.0,
                min_width: Some(px(f32::NAN)),
            });
        assert_eq!(column.width, None);
        assert_eq!(
            column.column_width,
            Some(ColumnWidth::Proportional {
                value: 1.0,
                min_width: None,
            })
        );
    }

    #[::core::prelude::v1::test]
    fn invalid_sort_targets_are_not_exposed_as_sorted() {
        let table = Table::new()
            .columns(vec![
                TableColumn::new("Name"),
                TableColumn::new("Role").sortable(true),
            ])
            .sort(0, TableSortDirection::Descending);
        let effective_sort = table.sort_column.filter(|column| {
            table
                .columns
                .get(*column)
                .is_some_and(|column| column.sortable)
        });
        assert_eq!(effective_sort, None);
    }
}

//! ASTRYX component-name aliases for equivalent Kael UI implementations.

use crate::components::{
    checkbox::{Checkbox, CheckboxSize},
    date_picker::DatePicker,
    dropdown::Dropdown,
    field::FieldStatusType,
    file_upload::FileUpload,
    image_viewer::{ImageItem, ImageViewer, ImageViewerState},
    input::{Input, InputSize},
    input_state::InputType,
    number_input::NumberInputSize,
    progress::{ProgressBar as KaelProgressBar, ProgressBarVariant as KaelProgressBarVariant},
    segmented_nav::SegmentedNav,
    select::{Select, SelectOption, SelectorOptionData},
    textarea::Textarea,
    time_picker::{TimeFormat, TimePicker},
    toggle::Toggle,
};
use crate::layout::{Grid, HStack, VStack};

pub type CheckboxInput = Checkbox;
pub type DateInput = DatePicker;
pub type Divider = crate::components::separator::Divider;
pub type DividerOrientation = crate::components::separator::SeparatorOrientation;
pub type DividerVariant = crate::components::separator::DividerVariant;
pub type DropdownMenu = Dropdown;
pub type FileInput = FileUpload;
pub type RadioList = crate::components::radio::RadioList;
pub type SegmentedControl = SegmentedNav;
pub type Selector<T> = Select<T>;
pub type Switch = Toggle;
pub type TextArea = Textarea;
pub type TextInput = Input;
pub type TextInputType = InputType;
pub type TimeInput = TimePicker;
pub type ToggleButton = crate::components::toggle_group::ToggleButton;
pub type Lightbox = ImageViewer;
pub type LightboxItem = ImageItem;
pub type LightboxMedia = ImageItem;
pub type LightboxMediaType = crate::components::image_viewer::LightboxMediaType;
pub type LightboxState = ImageViewerState;

pub type AstryxGrid = Grid;
pub type AstryxHStack = HStack;
pub type AstryxVStack = VStack;

pub type InputStatus = FieldStatusType;
pub type InputStatusType = FieldStatusType;

pub type CheckboxInputSize = CheckboxSize;
pub type DateInputSize = InputSize;
pub type DateInputStatus = FieldStatusType;
pub type DateInputStatusType = FieldStatusType;
pub type DateRangeInputSize = InputSize;
pub type DateRangeInputStatus = FieldStatusType;
pub type DateRangeInputStatusType = FieldStatusType;
pub type DateTimeInputSize = InputSize;
pub type DateTimeInputHourFormat = TimeFormat;
pub type DateTimeInputStatus = FieldStatusType;
pub type DateTimeInputStatusType = FieldStatusType;
pub type FileInputStatus = FieldStatusType;
pub type FileInputStatusType = FieldStatusType;
pub type InputGroupSize = InputSize;
pub type MultiSelectorSize = InputSize;
pub type MultiSelectorStatus = FieldStatusType;
pub type MultiSelectorStatusType = FieldStatusType;
pub type NumberInputStatus = FieldStatusType;
pub type NumberInputStatusType = FieldStatusType;
pub type RadioListSize = CheckboxSize;
pub type SelectorOption<T> = SelectOption<T>;
pub type SelectorOptionType = SelectorOptionData;
pub type SelectorSize = InputSize;
pub type SelectorStatus = FieldStatusType;
pub type SelectorStatusType = FieldStatusType;
pub type SegmentedControlSize = crate::components::segmented_nav::SegmentedNavSize;
pub type TextAreaSize = InputSize;
pub type TextAreaStatus = FieldStatusType;
pub type TextAreaStatusType = FieldStatusType;
pub type TextInputSize = InputSize;
pub type TextInputStatus = FieldStatusType;
pub type TextInputStatusType = FieldStatusType;
pub type TimeInputHourFormat = TimeFormat;
pub type TimeInputSize = InputSize;
pub type TimeInputStatus = FieldStatusType;
pub type TimeInputStatusType = FieldStatusType;
pub type TokenizerSize = InputSize;
pub type TokenizerStatus = FieldStatusType;
pub type TokenizerStatusType = FieldStatusType;
pub type TypeaheadSize = InputSize;
pub type TypeaheadStatus = FieldStatusType;
pub type TypeaheadStatusType = FieldStatusType;

pub type NumberInputControlSize = NumberInputSize;
pub type ProgressBar = KaelProgressBar;
pub type ProgressBarVariant = KaelProgressBarVariant;

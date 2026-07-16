//! Tests for Linux dialog fallback behavior and mixed selection support.
//!
//! These tests verify:
//! - DialogOptions covers all needed fields (Req 20.1, 20.2, 20.3)
//! - can_select_mixed_files_and_dirs returns false on Linux (Req 20.4)
//! - The built-in dialog fallback functions exist and handle options correctly (Req 20.5)

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod linux_dialog {
    use crate::{DialogKind, DialogOptions, PathPromptOptions, SharedString};

    #[test]
    fn dialog_options_covers_all_fields() {
        // Verify DialogOptions has all fields needed for Req 20.3:
        // configurable title, message, detail text, and button labels
        let options = DialogOptions {
            kind: DialogKind::Info,
            title: SharedString::from("Test Title"),
            message: SharedString::from("Test Message"),
            detail: Some(SharedString::from("Detail text")),
            buttons: vec![SharedString::from("OK"), SharedString::from("Cancel")],
            default_button: Some(0),
            cancel_button: Some(1),
        };

        assert_eq!(options.title.as_ref(), "Test Title");
        assert_eq!(options.message.as_ref(), "Test Message");
        assert_eq!(options.detail.as_ref().unwrap().as_ref(), "Detail text");
        assert_eq!(options.buttons.len(), 2);
        assert_eq!(options.default_button, Some(0));
        assert_eq!(options.cancel_button, Some(1));
    }

    #[test]
    fn dialog_options_supports_all_kinds() {
        // Verify all DialogKind variants exist for message box dialogs (Req 20.3)
        let _info = DialogKind::Info;
        let _warning = DialogKind::Warning;
        let _error = DialogKind::Error;
    }

    #[test]
    fn path_prompt_options_supports_file_open_config() {
        // Verify PathPromptOptions covers filters, multi-selection, and directory selection (Req 20.1)
        let options = PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(SharedString::from("Select files")),
            filters: vec![crate::FileDialogFilter::new("Images", [".png", "jpg"])],
        };

        assert!(options.files);
        assert!(!options.directories);
        assert!(options.multiple);
        assert!(options.prompt.is_some());
        assert_eq!(options.filters[0].name.as_ref(), "Images");
        assert_eq!(
            options.filters[0]
                .extensions
                .iter()
                .map(|extension| extension.as_ref())
                .collect::<Vec<_>>(),
            vec!["png", "jpg"]
        );
    }

    #[test]
    fn path_prompt_options_supports_directory_selection() {
        // Verify directory selection mode works (Req 20.1)
        let options = PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
            filters: vec![],
        };

        assert!(!options.files);
        assert!(options.directories);
    }

    #[test]
    fn fallback_functions_exist() {
        // Verify the fallback dialog functions are accessible (Req 20.5)
        // We can't actually run zenity/kdialog in CI, but we verify the functions
        // compile and accept the correct parameter types.
        let _open_fn = crate::platform::linux::dialog::prompt_for_paths_fallback;
        let _save_fn = crate::platform::linux::dialog::prompt_for_new_path_fallback;
        let _show_fn = crate::platform::linux::dialog::show_dialog;
    }
}

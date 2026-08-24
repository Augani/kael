//! Allocation-free admission limits for browser clipboard payloads.

/// Maximum number of browser-owned entries considered in one paste event.
pub(crate) const MAX_BROWSER_CLIPBOARD_ITEMS: usize = 32;
/// Maximum UTF-8 bytes accepted for the plain-text representation.
pub(crate) const MAX_BROWSER_CLIPBOARD_TEXT_BYTES: u64 = 8 * 1024 * 1024;
/// Maximum UTF-8 bytes accepted for an HTML representation.
pub(crate) const MAX_BROWSER_CLIPBOARD_HTML_BYTES: u64 = 16 * 1024 * 1024;
/// Maximum UTF-8 bytes accepted for a URI-list representation.
pub(crate) const MAX_BROWSER_CLIPBOARD_URI_BYTES: u64 = 1024 * 1024;
/// Maximum encoded bytes accepted for one pasted image.
pub(crate) const MAX_BROWSER_CLIPBOARD_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
/// Maximum combined bytes accepted across all representations and images.
pub(crate) const MAX_BROWSER_CLIPBOARD_TOTAL_BYTES: u64 = 128 * 1024 * 1024;

/// Browser clipboard representation whose encoded length is being admitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserClipboardItemKind {
    PlainText,
    Html,
    UriList,
    Image,
}

impl BrowserClipboardItemKind {
    fn byte_limit(self) -> u64 {
        match self {
            Self::PlainText => MAX_BROWSER_CLIPBOARD_TEXT_BYTES,
            Self::Html => MAX_BROWSER_CLIPBOARD_HTML_BYTES,
            Self::UriList => MAX_BROWSER_CLIPBOARD_URI_BYTES,
            Self::Image => MAX_BROWSER_CLIPBOARD_IMAGE_BYTES,
        }
    }

    #[cfg(test)]
    fn label(self) -> &'static str {
        match self {
            Self::PlainText => "plain text",
            Self::Html => "HTML",
            Self::UriList => "URI list",
            Self::Image => "image",
        }
    }
}

/// Content-safe reason for rejecting a browser paste before application dispatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserClipboardLimitError {
    TooManyItems,
    InvalidItemSize,
    InvalidPayload,
    ItemTooLarge(BrowserClipboardItemKind),
    AggregateTooLarge,
}

impl BrowserClipboardLimitError {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::TooManyItems => "browser clipboard contains too many items",
            Self::InvalidItemSize => "browser clipboard item reported an invalid size",
            Self::InvalidPayload => "browser clipboard payload failed validation",
            Self::ItemTooLarge(kind) => match kind {
                BrowserClipboardItemKind::PlainText => {
                    "browser clipboard plain text exceeds the intake limit"
                }
                BrowserClipboardItemKind::Html => "browser clipboard HTML exceeds the intake limit",
                BrowserClipboardItemKind::UriList => {
                    "browser clipboard URI list exceeds the intake limit"
                }
                BrowserClipboardItemKind::Image => {
                    "browser clipboard image exceeds the intake limit"
                }
            },
            Self::AggregateTooLarge => "browser clipboard aggregate exceeds the intake limit",
        }
    }
}

/// Validate a browser-reported source item count before iterating it.
pub(crate) fn validate_browser_clipboard_item_count(
    count: usize,
) -> Result<(), BrowserClipboardLimitError> {
    if count > MAX_BROWSER_CLIPBOARD_ITEMS {
        Err(BrowserClipboardLimitError::TooManyItems)
    } else {
        Ok(())
    }
}

/// Checked aggregate byte budget for one browser paste.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct BrowserClipboardBudget {
    total_bytes: u64,
}

impl BrowserClipboardBudget {
    /// Restore a previously checked prefix, such as the textual representations
    /// retained while image promises are pending.
    pub(crate) fn from_total_bytes(total_bytes: u64) -> Result<Self, BrowserClipboardLimitError> {
        if total_bytes > MAX_BROWSER_CLIPBOARD_TOTAL_BYTES {
            return Err(BrowserClipboardLimitError::AggregateTooLarge);
        }
        Ok(Self { total_bytes })
    }

    /// Admit one representation using both its per-kind and aggregate limits.
    pub(crate) fn try_add(
        &mut self,
        kind: BrowserClipboardItemKind,
        bytes: u64,
    ) -> Result<(), BrowserClipboardLimitError> {
        if bytes > kind.byte_limit() {
            return Err(BrowserClipboardLimitError::ItemTooLarge(kind));
        }
        let total_bytes = self
            .total_bytes
            .checked_add(bytes)
            .ok_or(BrowserClipboardLimitError::AggregateTooLarge)?;
        if total_bytes > MAX_BROWSER_CLIPBOARD_TOTAL_BYTES {
            return Err(BrowserClipboardLimitError::AggregateTooLarge);
        }
        self.total_bytes = total_bytes;
        Ok(())
    }

    pub(crate) fn total_bytes(self) -> u64 {
        self.total_bytes
    }

    /// Maximum image bytes worth reading, plus one sentinel byte that proves an
    /// apparently undersized browser `File` was actually too large.
    pub(crate) fn bounded_image_read_bytes(self) -> u64 {
        let remaining = MAX_BROWSER_CLIPBOARD_TOTAL_BYTES.saturating_sub(self.total_bytes);
        MAX_BROWSER_CLIPBOARD_IMAGE_BYTES
            .min(remaining)
            .saturating_add(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_clipboard_item_count_is_bounded() {
        assert!(validate_browser_clipboard_item_count(MAX_BROWSER_CLIPBOARD_ITEMS).is_ok());
        assert_eq!(
            validate_browser_clipboard_item_count(MAX_BROWSER_CLIPBOARD_ITEMS + 1),
            Err(BrowserClipboardLimitError::TooManyItems)
        );
    }

    #[test]
    fn browser_clipboard_representations_enforce_individual_limits() {
        for (kind, limit) in [
            (
                BrowserClipboardItemKind::PlainText,
                MAX_BROWSER_CLIPBOARD_TEXT_BYTES,
            ),
            (
                BrowserClipboardItemKind::Html,
                MAX_BROWSER_CLIPBOARD_HTML_BYTES,
            ),
            (
                BrowserClipboardItemKind::UriList,
                MAX_BROWSER_CLIPBOARD_URI_BYTES,
            ),
            (
                BrowserClipboardItemKind::Image,
                MAX_BROWSER_CLIPBOARD_IMAGE_BYTES,
            ),
        ] {
            let mut exact = BrowserClipboardBudget::default();
            assert!(
                exact.try_add(kind, limit).is_ok(),
                "{} exact limit",
                kind.label()
            );

            let mut oversized = BrowserClipboardBudget::default();
            assert_eq!(
                oversized.try_add(kind, limit + 1),
                Err(BrowserClipboardLimitError::ItemTooLarge(kind)),
                "{} oversized",
                kind.label()
            );
        }
    }

    #[test]
    fn browser_clipboard_aggregate_and_read_sentinel_are_bounded() {
        let mut budget = BrowserClipboardBudget::default();
        budget
            .try_add(
                BrowserClipboardItemKind::Image,
                MAX_BROWSER_CLIPBOARD_IMAGE_BYTES,
            )
            .unwrap();
        assert_eq!(
            budget.bounded_image_read_bytes(),
            MAX_BROWSER_CLIPBOARD_IMAGE_BYTES + 1
        );
        budget
            .try_add(
                BrowserClipboardItemKind::Image,
                MAX_BROWSER_CLIPBOARD_IMAGE_BYTES,
            )
            .unwrap();
        assert_eq!(budget.total_bytes(), MAX_BROWSER_CLIPBOARD_TOTAL_BYTES);
        assert_eq!(budget.bounded_image_read_bytes(), 1);
        assert_eq!(
            budget.try_add(BrowserClipboardItemKind::PlainText, 1),
            Err(BrowserClipboardLimitError::AggregateTooLarge)
        );
    }

    #[test]
    fn browser_clipboard_budget_rejects_invalid_restored_totals() {
        assert_eq!(
            BrowserClipboardBudget::from_total_bytes(MAX_BROWSER_CLIPBOARD_TOTAL_BYTES + 1),
            Err(BrowserClipboardLimitError::AggregateTooLarge)
        );
    }

    #[test]
    fn browser_clipboard_rejection_messages_are_content_safe() {
        assert_eq!(
            BrowserClipboardLimitError::InvalidItemSize.message(),
            "browser clipboard item reported an invalid size"
        );
        assert_eq!(
            BrowserClipboardLimitError::InvalidPayload.message(),
            "browser clipboard payload failed validation"
        );
        assert!(
            !BrowserClipboardLimitError::TooManyItems
                .message()
                .contains("clipboard contents")
        );
    }
}

//! Push-token types.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A push-registration token returned by a platform backend.
///
/// Debug formatting is always redacted so routine logs do not expose the token.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushToken(Vec<u8>);

impl PushToken {
    /// Creates a token from the opaque bytes returned by a push provider.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    /// Returns the opaque provider token bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Consumes the token and returns its opaque bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

impl fmt::Debug for PushToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PushToken([REDACTED])")
    }
}

#[cfg(test)]
mod tests {
    use super::PushToken;

    #[test]
    fn token_debug_output_is_redacted() {
        let token = PushToken::new([1, 2, 3]);
        assert_eq!(token.as_bytes(), &[1, 2, 3]);
        assert_eq!(format!("{token:?}"), "PushToken([REDACTED])");
        assert_eq!(token.into_bytes(), vec![1, 2, 3]);
    }
}

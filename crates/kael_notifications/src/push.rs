//! Push-token types.

use serde::{Deserialize, Serialize};

/// A push-registration token returned by a platform backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushToken(pub Vec<u8>);

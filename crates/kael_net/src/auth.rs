use std::collections::HashMap;
use std::fmt;

use kael_secrets::SecretStore;

/// An authentication token for a named service.
#[derive(Clone, serde::Serialize, serde::Deserialize, zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
pub struct AuthToken {
    /// The raw token string.
    pub token: String,
    /// The token type (e.g. "Bearer", "Basic").
    pub token_type: String,
    /// Optional Unix timestamp (seconds) when the token expires.
    pub expires_at: Option<u64>,
}

impl fmt::Debug for AuthToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthToken")
            .field("token", &"<redacted>")
            .field("token_type", &self.token_type)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// In-memory token storage keyed by service name.
#[derive(Debug, Clone)]
pub struct TokenStore {
    tokens: HashMap<String, AuthToken>,
}

impl TokenStore {
    /// Create an empty token store.
    pub fn new() -> Self {
        Self {
            tokens: HashMap::new(),
        }
    }

    /// Store or replace a token for the given service.
    pub fn set(&mut self, service: impl Into<String>, token: AuthToken) {
        self.tokens.insert(service.into(), token);
    }

    /// Retrieve a token for the given service.
    pub fn get(&self, service: &str) -> Option<&AuthToken> {
        self.tokens.get(service)
    }

    /// Remove and return the token for the given service.
    pub fn remove(&mut self, service: &str) -> Option<AuthToken> {
        self.tokens.remove(service)
    }

    /// Check whether the token for a service has expired given the current time.
    ///
    /// Returns `true` if the token does not exist or has a set expiry in the past.
    /// Returns `false` if the token exists and has no expiry or its expiry is in the future.
    pub fn is_expired(&self, service: &str, now: u64) -> bool {
        match self.tokens.get(service) {
            None => true,
            Some(token) => match token.expires_at {
                None => false,
                Some(expires) => now >= expires,
            },
        }
    }

    /// Remove all stored tokens.
    pub fn clear(&mut self) {
        self.tokens.clear();
    }

    /// Return the number of stored tokens.
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// Return true if no tokens are stored.
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }
}

impl Default for TokenStore {
    fn default() -> Self {
        Self::new()
    }
}

/// A persistent token store backed by an OS keychain via [`kael_secrets`].
///
/// Tokens are JSON-serialized and stored as secrets under a single keychain
/// service namespace, keyed by the logical service name. Unlike [`TokenStore`]
/// (in-memory only), credentials stored here survive restarts and are protected
/// by the platform secret store rather than held in plaintext.
pub struct SecureTokenStore {
    keychain_service: String,
    store: Box<dyn SecretStore>,
}

impl SecureTokenStore {
    /// Create a store that persists tokens under `keychain_service` using the
    /// provided secret backend.
    pub fn new(keychain_service: impl Into<String>, store: Box<dyn SecretStore>) -> Self {
        Self {
            keychain_service: keychain_service.into(),
            store,
        }
    }

    /// Create a store using the platform's default secret backend (the OS
    /// keychain where available).
    pub fn with_default_store(keychain_service: impl Into<String>) -> anyhow::Result<Self> {
        Ok(Self::new(keychain_service, kael_secrets::default_store()?))
    }

    /// Persist `token` for `service`, replacing any existing value.
    pub fn set(&self, service: &str, token: &AuthToken) -> anyhow::Result<()> {
        let json = zeroize::Zeroizing::new(serde_json::to_vec(token)?);
        self.store
            .set_secret(&self.keychain_service, service, &json)
    }

    /// Load the token for `service`, or `None` if absent.
    pub fn get(&self, service: &str) -> anyhow::Result<Option<AuthToken>> {
        match self.store.get_secret(&self.keychain_service, service)? {
            Some(bytes) => Ok(Some(serde_json::from_slice(bytes.expose_secret())?)),
            None => Ok(None),
        }
    }

    /// Delete the token for `service`. Succeeds even if absent.
    pub fn remove(&self, service: &str) -> anyhow::Result<()> {
        self.store.delete_secret(&self.keychain_service, service)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_token(token: &str, expires_at: Option<u64>) -> AuthToken {
        AuthToken {
            token: token.to_string(),
            token_type: "Bearer".to_string(),
            expires_at,
        }
    }

    #[test]
    fn test_new_store() {
        let store = TokenStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn test_default_store() {
        let store = TokenStore::default();
        assert!(store.is_empty());
    }

    #[test]
    fn debug_redacts_token() {
        let token = make_token("secret-token", None);
        let debug = format!("{token:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("secret-token"));
    }

    #[test]
    fn test_set_and_get() {
        let mut store = TokenStore::new();
        store.set("github", make_token("gh_abc123", None));
        let token = store.get("github").unwrap();
        assert_eq!(token.token, "gh_abc123");
        assert_eq!(token.token_type, "Bearer");
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn test_set_overwrites() {
        let mut store = TokenStore::new();
        store.set("github", make_token("old_token", None));
        store.set("github", make_token("new_token", None));
        assert_eq!(store.get("github").unwrap().token, "new_token");
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn test_get_nonexistent() {
        let store = TokenStore::new();
        assert!(store.get("nonexistent").is_none());
    }

    #[test]
    fn test_remove() {
        let mut store = TokenStore::new();
        store.set("github", make_token("token", None));
        let removed = store.remove("github").unwrap();
        assert_eq!(removed.token, "token");
        assert!(store.is_empty());
    }

    #[test]
    fn test_remove_nonexistent() {
        let mut store = TokenStore::new();
        assert!(store.remove("nonexistent").is_none());
    }

    #[test]
    fn test_is_expired_no_token() {
        let store = TokenStore::new();
        assert!(store.is_expired("github", 1000));
    }

    #[test]
    fn test_is_expired_no_expiry() {
        let mut store = TokenStore::new();
        store.set("github", make_token("token", None));
        assert!(!store.is_expired("github", 1000));
    }

    #[test]
    fn test_is_expired_future_expiry() {
        let mut store = TokenStore::new();
        store.set("github", make_token("token", Some(2000)));
        assert!(!store.is_expired("github", 1000));
    }

    #[test]
    fn test_is_expired_past_expiry() {
        let mut store = TokenStore::new();
        store.set("github", make_token("token", Some(500)));
        assert!(store.is_expired("github", 1000));
    }

    #[test]
    fn test_is_expired_exact_boundary() {
        let mut store = TokenStore::new();
        store.set("github", make_token("token", Some(1000)));
        assert!(store.is_expired("github", 1000));
    }

    #[test]
    fn test_clear() {
        let mut store = TokenStore::new();
        store.set("github", make_token("a", None));
        store.set("gitlab", make_token("b", None));
        store.clear();
        assert!(store.is_empty());
    }

    #[test]
    fn test_multiple_services() {
        let mut store = TokenStore::new();
        store.set("github", make_token("gh", None));
        store.set("gitlab", make_token("gl", None));
        store.set("bitbucket", make_token("bb", None));
        assert_eq!(store.len(), 3);
        assert_eq!(store.get("github").unwrap().token, "gh");
        assert_eq!(store.get("gitlab").unwrap().token, "gl");
        assert_eq!(store.get("bitbucket").unwrap().token, "bb");
    }

    #[test]
    fn secure_store_round_trips() {
        let store = SecureTokenStore::new(
            "com.kael.test",
            Box::new(kael_secrets::MemorySecretStore::new()),
        );
        assert!(store.get("github").unwrap().is_none());

        store
            .set("github", &make_token("gh_secret", Some(123)))
            .unwrap();
        let loaded = store.get("github").unwrap().unwrap();
        assert_eq!(loaded.token, "gh_secret");
        assert_eq!(loaded.expires_at, Some(123));

        store
            .set("github", &make_token("gh_rotated", None))
            .unwrap();
        assert_eq!(store.get("github").unwrap().unwrap().token, "gh_rotated");

        store.remove("github").unwrap();
        assert!(store.get("github").unwrap().is_none());
    }

    #[test]
    fn secure_store_isolates_services() {
        let store = SecureTokenStore::new("ns", Box::new(kael_secrets::MemorySecretStore::new()));
        store.set("a", &make_token("ta", None)).unwrap();
        store.set("b", &make_token("tb", None)).unwrap();
        assert_eq!(store.get("a").unwrap().unwrap().token, "ta");
        assert_eq!(store.get("b").unwrap().unwrap().token, "tb");
        assert!(store.get("c").unwrap().is_none());
    }
}

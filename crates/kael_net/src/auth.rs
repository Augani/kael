use std::collections::HashMap;

/// An authentication token for a named service.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuthToken {
    /// The raw token string.
    pub token: String,
    /// The token type (e.g. "Bearer", "Basic").
    pub token_type: String,
    /// Optional Unix timestamp (seconds) when the token expires.
    pub expires_at: Option<u64>,
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
}

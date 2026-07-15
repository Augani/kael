//! Secure credential storage backed by the operating-system keychain.
//!
//! Replaces ad-hoc in-memory/plaintext token storage with the platform secret
//! store: the macOS Keychain, the Windows Credential Manager, and the Linux
//! freedesktop Secret Service, with an in-process [`MemorySecretStore`] fallback
//! elsewhere. Use [`default_store`] to obtain the best available store for the
//! current platform.

#![deny(missing_docs)]

use std::collections::HashMap;

use anyhow::Result;
use parking_lot::Mutex;
use zeroize::Zeroizing;

const MAX_IDENTIFIER_BYTES: usize = 4 * 1024;
const MAX_SECRET_BYTES: usize = 16 * 1024 * 1024;

fn validate_address(service: &str, account: &str) -> Result<()> {
    validate_identifier("service", service)?;
    validate_identifier("account", account)
}

fn validate_identifier(label: &str, value: &str) -> Result<()> {
    anyhow::ensure!(!value.is_empty(), "secret {label} must not be empty");
    anyhow::ensure!(
        !value.contains('\0'),
        "secret {label} must not contain a NUL character"
    );
    anyhow::ensure!(
        value.len() <= MAX_IDENTIFIER_BYTES,
        "secret {label} exceeds the {MAX_IDENTIFIER_BYTES} byte limit"
    );
    Ok(())
}

fn validate_secret(secret: &[u8]) -> Result<()> {
    validate_secret_len(secret.len())
}

fn validate_secret_len(secret_len: usize) -> Result<()> {
    anyhow::ensure!(
        secret_len <= MAX_SECRET_BYTES,
        "secret exceeds the {MAX_SECRET_BYTES} byte limit"
    );
    Ok(())
}

/// A keyed secret store. Secrets are addressed by a `(service, account)` pair.
pub trait SecretStore: Send + Sync {
    /// Store `secret` for `(service, account)`, replacing any existing value.
    fn set_secret(&self, service: &str, account: &str, secret: &[u8]) -> Result<()>;

    /// Retrieve the secret for `(service, account)`, or `None` if absent.
    fn get_secret(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>>;

    /// Delete the secret for `(service, account)`. Succeeds even if absent.
    fn delete_secret(&self, service: &str, account: &str) -> Result<()>;

    /// Convenience: store a UTF-8 string secret.
    fn set_string(&self, service: &str, account: &str, secret: &str) -> Result<()> {
        self.set_secret(service, account, secret.as_bytes())
    }

    /// Convenience: retrieve a secret as a UTF-8 string.
    fn get_string(&self, service: &str, account: &str) -> Result<Option<String>> {
        match self.get_secret(service, account)? {
            Some(bytes) => Ok(Some(String::from_utf8(bytes)?)),
            None => Ok(None),
        }
    }
}

/// An in-process, non-persistent [`SecretStore`].
///
/// Used as the fallback on platforms without a wired native backend and for
/// tests. Secrets live only for the lifetime of the process.
#[derive(Default)]
pub struct MemorySecretStore {
    entries: Mutex<HashMap<(String, String), Zeroizing<Vec<u8>>>>,
}

impl MemorySecretStore {
    /// Create an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemorySecretStore {
    fn set_secret(&self, service: &str, account: &str, secret: &[u8]) -> Result<()> {
        validate_address(service, account)?;
        validate_secret(secret)?;
        self.entries.lock().insert(
            (service.to_string(), account.to_string()),
            Zeroizing::new(secret.to_vec()),
        );
        Ok(())
    }

    fn get_secret(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>> {
        validate_address(service, account)?;
        Ok(self
            .entries
            .lock()
            .get(&(service.to_string(), account.to_string()))
            .map(|secret| secret.to_vec()))
    }

    fn delete_secret(&self, service: &str, account: &str) -> Result<()> {
        validate_address(service, account)?;
        self.entries
            .lock()
            .remove(&(service.to_string(), account.to_string()));
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod keychain {
    use super::{Result, SecretStore, validate_address, validate_secret};
    use anyhow::anyhow;

    const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

    /// A [`SecretStore`] backed by the macOS Keychain (generic password items).
    pub struct KeychainSecretStore;

    impl SecretStore for KeychainSecretStore {
        fn set_secret(&self, service: &str, account: &str, secret: &[u8]) -> Result<()> {
            validate_address(service, account)?;
            validate_secret(secret)?;
            security_framework::passwords::set_generic_password(service, account, secret)
                .map_err(|error| anyhow!("keychain write failed: {error}"))
        }

        fn get_secret(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>> {
            validate_address(service, account)?;
            match security_framework::passwords::get_generic_password(service, account) {
                Ok(secret) => Ok(Some(secret)),
                Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
                Err(error) => Err(anyhow!("keychain read failed: {error}")),
            }
        }

        fn delete_secret(&self, service: &str, account: &str) -> Result<()> {
            validate_address(service, account)?;
            match security_framework::passwords::delete_generic_password(service, account) {
                Ok(()) => Ok(()),
                Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
                Err(error) => Err(anyhow!("keychain delete failed: {error}")),
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use keychain::KeychainSecretStore;

#[cfg(target_os = "windows")]
mod windows_backend {
    use super::{Result, SecretStore, validate_address, validate_secret};
    use anyhow::anyhow;
    use windows::Win32::Foundation::ERROR_NOT_FOUND;
    use windows::Win32::Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree,
        CredReadW, CredWriteW,
    };
    use windows::core::{PCWSTR, PWSTR};
    use zeroize::Zeroizing;

    /// A [`SecretStore`] backed by the Windows Credential Manager (generic credentials).
    pub struct CredentialStore;

    const MAX_CREDENTIAL_BLOB_BYTES: usize = 5 * 512;

    fn target_name(service: &str, account: &str) -> Vec<u16> {
        format!("{}:{service}{account}", service.encode_utf16().count())
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect()
    }

    impl SecretStore for CredentialStore {
        fn set_secret(&self, service: &str, account: &str, secret: &[u8]) -> Result<()> {
            validate_address(service, account)?;
            validate_secret(secret)?;
            anyhow::ensure!(
                secret.len() <= MAX_CREDENTIAL_BLOB_BYTES,
                "secret exceeds the Windows Credential Manager {MAX_CREDENTIAL_BLOB_BYTES} byte limit"
            );
            let mut name = target_name(service, account);
            let mut blob = Zeroizing::new(secret.to_vec());
            let blob_size = u32::try_from(blob.len())
                .map_err(|_| anyhow!("credential blob length does not fit in u32"))?;
            let credential = CREDENTIALW {
                Type: CRED_TYPE_GENERIC,
                TargetName: PWSTR(name.as_mut_ptr()),
                CredentialBlobSize: blob_size,
                CredentialBlob: blob.as_mut_ptr(),
                Persist: CRED_PERSIST_LOCAL_MACHINE,
                ..Default::default()
            };
            unsafe {
                CredWriteW(&credential, 0)
                    .map_err(|error| anyhow!("credential write failed: {error}"))
            }
        }

        fn get_secret(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>> {
            validate_address(service, account)?;
            let name = target_name(service, account);
            let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
            unsafe {
                match CredReadW(
                    PCWSTR(name.as_ptr()),
                    CRED_TYPE_GENERIC,
                    None,
                    &mut credential,
                ) {
                    Ok(()) => {
                        let guard = CredentialGuard(credential);
                        let cred = guard
                            .0
                            .as_ref()
                            .ok_or_else(|| anyhow!("credential read returned a null pointer"))?;
                        let blob_size = usize::try_from(cred.CredentialBlobSize)
                            .map_err(|_| anyhow!("credential blob length does not fit in usize"))?;
                        let bytes = if blob_size == 0 {
                            Vec::new()
                        } else {
                            anyhow::ensure!(
                                !cred.CredentialBlob.is_null(),
                                "credential read returned a null blob"
                            );
                            std::slice::from_raw_parts(cred.CredentialBlob, blob_size).to_vec()
                        };
                        Ok(Some(bytes))
                    }
                    Err(error) if error.code() == ERROR_NOT_FOUND.to_hresult() => Ok(None),
                    Err(error) => Err(anyhow!("credential read failed: {error}")),
                }
            }
        }

        fn delete_secret(&self, service: &str, account: &str) -> Result<()> {
            validate_address(service, account)?;
            let name = target_name(service, account);
            unsafe {
                match CredDeleteW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, None) {
                    Ok(()) => Ok(()),
                    Err(error) if error.code() == ERROR_NOT_FOUND.to_hresult() => Ok(()),
                    Err(error) => Err(anyhow!("credential delete failed: {error}")),
                }
            }
        }
    }

    struct CredentialGuard(*mut CREDENTIALW);

    impl Drop for CredentialGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    CredFree(self.0.cast());
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub use windows_backend::CredentialStore;

#[cfg(target_os = "linux")]
mod linux_backend {
    use std::collections::HashMap;

    use super::{Result, SecretStore, validate_address, validate_secret};
    use anyhow::anyhow;
    use secret_service::EncryptionType;
    use secret_service::blocking::SecretService;

    /// A [`SecretStore`] backed by the freedesktop Secret Service over D-Bus.
    pub struct SecretServiceStore;

    fn attributes<'a>(service: &'a str, account: &'a str) -> HashMap<&'a str, &'a str> {
        HashMap::from([("service", service), ("account", account)])
    }

    impl SecretStore for SecretServiceStore {
        fn set_secret(&self, service: &str, account: &str, secret: &[u8]) -> Result<()> {
            validate_address(service, account)?;
            validate_secret(secret)?;
            let session = SecretService::connect(EncryptionType::Dh)
                .map_err(|error| anyhow!("secret service connect failed: {error}"))?;
            let collection = session
                .get_default_collection()
                .map_err(|error| anyhow!("secret service collection failed: {error}"))?;
            collection
                .create_item(
                    &format!("{service}/{account}"),
                    attributes(service, account),
                    secret,
                    true,
                    "application/octet-stream",
                )
                .map_err(|error| anyhow!("secret service write failed: {error}"))?;
            Ok(())
        }

        fn get_secret(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>> {
            validate_address(service, account)?;
            let session = SecretService::connect(EncryptionType::Dh)
                .map_err(|error| anyhow!("secret service connect failed: {error}"))?;
            let result = session
                .search_items(attributes(service, account))
                .map_err(|error| anyhow!("secret service search failed: {error}"))?;
            let item = if let Some(item) = result.unlocked.first() {
                item
            } else if let Some(item) = result.locked.first() {
                item.unlock()
                    .map_err(|error| anyhow!("secret service unlock failed: {error}"))?;
                item
            } else {
                return Ok(None);
            };
            {
                let secret = item
                    .get_secret()
                    .map_err(|error| anyhow!("secret service read failed: {error}"))?;
                Ok(Some(secret))
            }
        }

        fn delete_secret(&self, service: &str, account: &str) -> Result<()> {
            validate_address(service, account)?;
            let session = SecretService::connect(EncryptionType::Dh)
                .map_err(|error| anyhow!("secret service connect failed: {error}"))?;
            let result = session
                .search_items(attributes(service, account))
                .map_err(|error| anyhow!("secret service search failed: {error}"))?;
            for item in result.unlocked.into_iter().chain(result.locked) {
                item.delete()
                    .map_err(|error| anyhow!("secret service delete failed: {error}"))?;
            }
            Ok(())
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux_backend::SecretServiceStore;

/// Return the best available [`SecretStore`] for the current platform.
///
/// macOS uses the Keychain, Windows the Credential Manager, and Linux the
/// freedesktop Secret Service; other platforms fall back to the in-process
/// [`MemorySecretStore`].
pub fn default_store() -> Box<dyn SecretStore> {
    #[cfg(target_os = "macos")]
    {
        Box::new(KeychainSecretStore)
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(CredentialStore)
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(SecretServiceStore)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Box::new(MemorySecretStore::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trips() {
        let store = MemorySecretStore::new();
        assert!(store.get_secret("svc", "acct").unwrap().is_none());

        store.set_secret("svc", "acct", b"token").unwrap();
        assert_eq!(
            store.get_secret("svc", "acct").unwrap().as_deref(),
            Some(&b"token"[..])
        );

        store.set_secret("svc", "acct", b"rotated").unwrap();
        assert_eq!(
            store.get_secret("svc", "acct").unwrap().as_deref(),
            Some(&b"rotated"[..])
        );

        store.delete_secret("svc", "acct").unwrap();
        assert!(store.get_secret("svc", "acct").unwrap().is_none());
    }

    #[test]
    fn memory_store_delete_is_idempotent() {
        let store = MemorySecretStore::new();
        store.delete_secret("svc", "missing").unwrap();
    }

    #[test]
    fn string_helpers_round_trip() {
        let store = MemorySecretStore::new();
        store.set_string("svc", "acct", "hello").unwrap();
        assert_eq!(
            store.get_string("svc", "acct").unwrap().as_deref(),
            Some("hello")
        );
    }

    #[test]
    fn entries_are_isolated_by_service_and_account() {
        let store = MemorySecretStore::new();
        store.set_secret("a", "x", b"1").unwrap();
        store.set_secret("a", "y", b"2").unwrap();
        store.set_secret("b", "x", b"3").unwrap();
        assert_eq!(
            store.get_secret("a", "x").unwrap().as_deref(),
            Some(&b"1"[..])
        );
        assert_eq!(
            store.get_secret("a", "y").unwrap().as_deref(),
            Some(&b"2"[..])
        );
        assert_eq!(
            store.get_secret("b", "x").unwrap().as_deref(),
            Some(&b"3"[..])
        );
    }

    #[test]
    fn rejects_ambiguous_addresses_and_oversized_secrets() {
        let store = MemorySecretStore::new();
        assert!(store.set_secret("", "account", b"secret").is_err());
        assert!(store.set_secret("service", "", b"secret").is_err());
        assert!(
            store
                .set_secret("service\0alias", "account", b"secret")
                .is_err()
        );
        assert!(validate_secret_len(MAX_SECRET_BYTES + 1).is_err());
    }

    #[test]
    fn default_store_is_usable() {
        let store = default_store();
        let _ = store.get_secret("kael-secrets-test", "probe");
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "touches the real login keychain; run manually with --ignored"]
    fn keychain_round_trip() {
        let store = KeychainSecretStore;
        let service = "com.kael.secrets.test";
        let account = "round-trip";
        let _ = store.delete_secret(service, account);

        store.set_secret(service, account, b"super-secret").unwrap();
        assert_eq!(
            store.get_secret(service, account).unwrap().as_deref(),
            Some(&b"super-secret"[..])
        );
        store.delete_secret(service, account).unwrap();
        assert!(store.get_secret(service, account).unwrap().is_none());
    }
}

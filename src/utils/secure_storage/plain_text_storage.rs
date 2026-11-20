//! Maps to: CC `utils/secureStorage/plainTextStorage.ts`.

use super::{SecureStorageBackend, SecureStorageData, SecureStorageUpdate};
use std::path::PathBuf;

pub(crate) struct PlainTextStorage;

fn update_credentials(
    data: &SecureStorageData,
    allowed: bool,
) -> anyhow::Result<SecureStorageUpdate> {
    let (storage_dir, storage_path) = get_storage_path();
    let serialized = match serde_json::to_string(data) {
        Ok(serialized) => serialized,
        Err(_) => return Ok(SecureStorageUpdate::default()),
    };
    if !allowed {
        return Err(crate::constants::oauth::OAuthCredentialSideEffectsUnavailable.into());
    }
    let updated = (|| -> std::io::Result<()> {
        if let Err(error) =
            crate::utils::fs_operations::get_fs_implementation().mkdir_sync(&storage_dir, None)
        {
            if crate::utils::errors::io_errno_code(&error) != Some("EEXIST") {
                return Err(error);
            }
        }
        std::fs::write(&storage_path, serialized)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&storage_path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    })()
    .is_ok();
    Ok(SecureStorageUpdate {
        success: updated,
        warning: updated.then(|| "Warning: Storing credentials in plaintext.".to_string()),
    })
}

fn delete_credentials(side_effects_enabled: bool) -> anyhow::Result<bool> {
    let (_, storage_path) = get_storage_path();
    if !side_effects_enabled {
        return Err(crate::constants::oauth::OAuthCredentialSideEffectsUnavailable.into());
    }
    match crate::utils::fs_operations::get_fs_implementation().unlink_sync(&storage_path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Maps to: CC `utils/secureStorage/plainTextStorage.ts:10-14` `getStoragePath`.
fn get_storage_path() -> (PathBuf, PathBuf) {
    let storage_dir = crate::utils::env_utils::get_claude_config_home_dir();
    let storage_path = storage_dir.join(".credentials.json");
    (storage_dir, storage_path)
}

impl SecureStorageBackend for PlainTextStorage {
    /// Maps to: CC `utils/secureStorage/plainTextStorage.ts:20` `name`.
    fn name(&self) -> &str {
        "plaintext"
    }

    /// Maps to: CC `utils/secureStorage/plainTextStorage.ts:21-32` `read`.
    fn read(&self) -> Option<SecureStorageData> {
        let (_, storage_path) = get_storage_path();
        crate::utils::auth::record_auth_io(crate::utils::auth::AuthIoOperation::TokenFileRead);
        crate::utils::fs_operations::get_fs_implementation()
            .read_file_sync(
                &storage_path,
                crate::utils::fs_operations::BufferEncoding::Utf8,
            )
            .map(|text| text.to_string_lossy())
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
    }

    /// Maps to: CC `utils/secureStorage/plainTextStorage.ts:44-68` `update`.
    fn update(&self, data: &SecureStorageData) -> anyhow::Result<SecureStorageUpdate> {
        // Deviation (L2, user-authorized OAuth safety gate): CC writes the
        // prepared plaintext credential object here. Directory creation,
        // bytes, and permissions remain untouched while the sole switch is off.
        update_credentials(
            data,
            crate::constants::oauth::OAUTH_CREDENTIAL_SIDE_EFFECTS_ENABLED,
        )
    }

    fn update_mcp_oauth_credentials(
        &self,
        data: &SecureStorageData,
    ) -> anyhow::Result<SecureStorageUpdate> {
        update_credentials(
            data,
            crate::constants::oauth::MCP_OAUTH_CREDENTIAL_SIDE_EFFECTS_ENABLED,
        )
    }

    /// Maps to: CC `utils/secureStorage/plainTextStorage.ts:70-84` `delete`.
    fn delete(&self) -> anyhow::Result<bool> {
        delete_credentials(crate::constants::oauth::OAUTH_CREDENTIAL_SIDE_EFFECTS_ENABLED)
    }

    fn delete_mcp_oauth_credentials(&self) -> anyhow::Result<bool> {
        delete_credentials(crate::constants::oauth::MCP_OAUTH_CREDENTIAL_SIDE_EFFECTS_ENABLED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test_env::{EnvVarGuard, TEST_ENV_LOCK};

    #[test]
    fn mcp_oauth_delete_uses_scoped_switch_without_enabling_account_oauth() {
        let _lock = TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let home =
            std::env::temp_dir().join(format!("cometix-mcp-storage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&home).unwrap();
        let _config_dir = EnvVarGuard::set("CLAUDE_CONFIG_DIR", &home);
        let path = home.join(".credentials.json");
        std::fs::write(&path, br#"{"mcpOAuth":{"test":{}}}"#).unwrap();

        assert!(!crate::constants::oauth::OAUTH_CREDENTIAL_SIDE_EFFECTS_ENABLED);
        assert!(PlainTextStorage.delete_mcp_oauth_credentials().unwrap());
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(home);
    }
}

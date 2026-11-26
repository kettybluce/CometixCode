//! Maps to: CC `utils/secureStorage/fallbackStorage.ts`.

use super::{SecureStorageBackend, SecureStorageData, SecureStorageUpdate};

pub(crate) struct FallbackStorage {
    name: String,
    primary: Box<dyn SecureStorageBackend>,
    secondary: Box<dyn SecureStorageBackend>,
}

impl SecureStorageBackend for FallbackStorage {
    /// Maps to: CC `utils/secureStorage/fallbackStorage.ts:12` returned `name`.
    fn name(&self) -> &str {
        &self.name
    }

    /// Maps to: CC `utils/secureStorage/fallbackStorage.ts:13-19` returned `read`.
    fn read(&self) -> Option<SecureStorageData> {
        self.primary
            .read()
            .or_else(|| self.secondary.read())
            .or_else(|| Some(serde_json::json!({})))
    }

    /// Maps to: CC `utils/secureStorage/fallbackStorage.ts:27-61` returned `update`.
    /// L2 (`OAuth credential side-effect closure`): a typed closed-outlet error
    /// is propagated and never converted into a secondary credential write.
    fn update(&self, data: &SecureStorageData) -> anyhow::Result<SecureStorageUpdate> {
        self.update_with(data, false)
    }

    fn update_mcp_oauth_credentials(
        &self,
        data: &SecureStorageData,
    ) -> anyhow::Result<SecureStorageUpdate> {
        self.update_with(data, true)
    }

    fn delete(&self) -> anyhow::Result<bool> {
        let primary_success = self.primary.delete()?;
        let secondary_success = self.secondary.delete()?;
        Ok(primary_success || secondary_success)
    }

    fn delete_for_logout(&self) -> anyhow::Result<bool> {
        let primary_success = self.primary.delete_for_logout()?;
        let secondary_success = self.secondary.delete_for_logout()?;
        Ok(primary_success || secondary_success)
    }

    fn delete_mcp_oauth_credentials(&self) -> anyhow::Result<bool> {
        let primary_success = self.primary.delete_mcp_oauth_credentials()?;
        let secondary_success = self.secondary.delete_mcp_oauth_credentials()?;
        Ok(primary_success || secondary_success)
    }
}

impl FallbackStorage {
    fn update_with(
        &self,
        data: &SecureStorageData,
        mcp_oauth: bool,
    ) -> anyhow::Result<SecureStorageUpdate> {
        let primary_data_before = self.primary.read();
        let primary_result = if mcp_oauth {
            self.primary.update_mcp_oauth_credentials(data)?
        } else {
            self.primary.update(data)?
        };
        if primary_result.success {
            if primary_data_before.is_none() {
                if mcp_oauth {
                    let _ = self.secondary.delete_mcp_oauth_credentials()?;
                } else {
                    let _ = self.secondary.delete()?;
                }
            }
            return Ok(primary_result);
        }

        let fallback_result = if mcp_oauth {
            self.secondary.update_mcp_oauth_credentials(data)?
        } else {
            self.secondary.update(data)?
        };
        if fallback_result.success {
            if primary_data_before.is_some() {
                if mcp_oauth {
                    let _ = self.primary.delete_mcp_oauth_credentials()?;
                } else {
                    let _ = self.primary.delete()?;
                }
            }
            return Ok(SecureStorageUpdate {
                success: true,
                warning: fallback_result.warning,
            });
        }

        Ok(SecureStorageUpdate {
            success: false,
            warning: None,
        })
    }
}

/// Maps to: CC `utils/secureStorage/fallbackStorage.ts:7-62`
/// `createFallbackStorage`.
pub(crate) fn create_fallback_storage(
    primary: Box<dyn SecureStorageBackend>,
    secondary: Box<dyn SecureStorageBackend>,
) -> FallbackStorage {
    let name = format!("{}-with-{}-fallback", primary.name(), secondary.name());
    FallbackStorage {
        name,
        primary,
        secondary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Calls {
        standard_delete: usize,
        mcp_delete: usize,
    }

    struct Backend {
        name: &'static str,
        update_success: bool,
        calls: Arc<Mutex<Calls>>,
    }

    impl SecureStorageBackend for Backend {
        fn name(&self) -> &str {
            self.name
        }

        fn read(&self) -> Option<SecureStorageData> {
            None
        }

        fn update(&self, _data: &SecureStorageData) -> anyhow::Result<SecureStorageUpdate> {
            Ok(SecureStorageUpdate {
                success: self.update_success,
                warning: None,
            })
        }

        fn update_mcp_oauth_credentials(
            &self,
            data: &SecureStorageData,
        ) -> anyhow::Result<SecureStorageUpdate> {
            self.update(data)
        }

        fn delete(&self) -> anyhow::Result<bool> {
            self.calls.lock().unwrap().standard_delete += 1;
            Ok(true)
        }

        fn delete_mcp_oauth_credentials(&self) -> anyhow::Result<bool> {
            self.calls.lock().unwrap().mcp_delete += 1;
            Ok(true)
        }
    }

    #[test]
    fn successful_mcp_keychain_update_clears_fallback_with_mcp_delete_outlet() {
        let calls = Arc::new(Mutex::new(Calls::default()));
        let storage = create_fallback_storage(
            Box::new(Backend {
                name: "keychain",
                update_success: true,
                calls: calls.clone(),
            }),
            Box::new(Backend {
                name: "plaintext",
                update_success: true,
                calls: calls.clone(),
            }),
        );

        let result = storage
            .update_mcp_oauth_credentials(&serde_json::json!({"mcpOAuth": {}}))
            .unwrap();
        assert!(result.success);
        let calls = calls.lock().unwrap();
        assert_eq!(calls.standard_delete, 0);
        assert_eq!(calls.mcp_delete, 1);
    }
}

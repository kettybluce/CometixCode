//! Maps to CC `commands/logout/logout.tsx`.

use crate::commands::Command;
use crate::components::design_system::dialog::Dialog;
use crate::tool::ToolUseContext;
use crate::utils::process_user_input::ProcessUserInputBaseResult;
use crate::utils::process_user_input::process_slash_command::{
    LocalCommandUi, SlashCommandAction, SlashCommandInvocation,
};
use crate::utils::theme::Theme;
use iocraft::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LogoutOutcome {
    pub removed_account_credentials: bool,
}

/// Source-shaped local JSX dispatch. The actual cache/credential operation is
/// completed by the REPL-owned panel callback so its transcript and authVersion
/// update stay in one UI turn.
pub fn dispatch(
    command: &Command,
    args: &str,
    _uuid: Option<String>,
    _context: &ToolUseContext,
) -> ProcessUserInputBaseResult {
    ProcessUserInputBaseResult {
        messages: Vec::new(),
        should_query: false,
        allowed_tools: None,
        local_action: Some(SlashCommandAction::OpenLocalCommandUi {
            command: LocalCommandUi::Logout {
                args: args.to_string(),
            },
            invocation: SlashCommandInvocation::new(command.name.as_ref(), args),
        }),
        query_source: crate::constants::query_source::QuerySource::Prompt,
    }
}

/// Maps to CC `commands/logout/logout.tsx#performLogout`.
///
/// Source cache invalidations that have a concrete Rust owner are kept here.
/// The explicit logout path uses the deletion-only secure-storage outlet, so it
/// stays usable while account credential writes and refreshes remain closed.
pub fn perform_logout(clear_onboarding: bool) -> anyhow::Result<()> {
    perform_logout_status(clear_onboarding).map(|_| ())
}

/// Shared by the shell `auth logout` command and the in-session `/logout`
/// panel. Returns whether an account credential was present before deletion;
/// the shell command uses that to reproduce the upstream "revocation not
/// confirmed" message when it cannot reach Anthropic.
pub fn perform_logout_status(clear_onboarding: bool) -> anyhow::Result<LogoutOutcome> {
    let storage = crate::utils::secure_storage::get_secure_storage();
    let removed_account_credentials = storage.read().is_some_and(|value| {
        value
            .get("claudeAiOauth")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|oauth| !oauth.is_empty())
    });
    storage.delete_for_logout()?;
    clear_auth_related_caches();
    crate::utils::config::save_global_config(|config| {
        config.oauth_account = None;
        if clear_onboarding {
            config.has_completed_onboarding = Some(false);
        }
    })?;
    Ok(LogoutOutcome {
        removed_account_credentials,
    })
}

/// Maps to CC `commands/logout/logout.tsx#clearAuthRelatedCaches` for the
/// cache families that have a source-owned Rust implementation. Unsupported
/// Grove, remote-managed, policy-limit and trusted-device caches are left at
/// their existing owners until those services expose the corresponding clear
/// operation.
pub fn clear_auth_related_caches() {
    crate::utils::auth::clear_api_key_helper_cache();
    crate::utils::betas::clear_betas_caches();
    crate::utils::tool_schema_cache::clear_tool_schema_cache();
}

#[derive(Default, Props)]
pub struct LogoutCommandProps<'a> {
    pub on_done: HandlerMut<'a, String>,
}

/// Maps to CC `commands/logout/logout.tsx#call`'s completion text. The source
/// schedules process shutdown after completion; REPL owns that lifecycle and
/// keeps the current process alive while the OAuth side-effect gate is closed.
#[component]
pub fn LogoutCommand<'a>(
    props: &mut LogoutCommandProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let theme = hooks.use_context::<Theme>();
    let mut started = hooks.use_state(|| false);
    let mut result = hooks.use_state(|| Option::<String>::None);
    if !started.get() {
        started.set(true);
        let output = match perform_logout(true) {
            Ok(()) => "Successfully logged out from your Anthropic account.".to_string(),
            Err(error) => format!("Logout unavailable: {error}"),
        };
        result.set(Some(output));
    }
    let output = result.read().clone();
    if let Some(output) = output {
        result.set(None);
        (props.on_done)(output);
    }
    element! {
        Dialog(
            title: "Logout".to_string(),
            color: Some(theme.permission),
            on_cancel: move |_| result.set(Some("Logout cancelled".to_string())),
        ) {
            Text(content: "Signing out from your Anthropic account…".to_string(), wrap: TextWrap::Wrap)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn logout_delete_stays_available_while_account_writes_are_closed() {
        let _lock = crate::utils::test_env::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let root =
            std::env::temp_dir().join(format!("cometix-logout-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let _config_dir = crate::utils::test_env::EnvVarGuard::set("CLAUDE_CONFIG_DIR", &root);
        let credentials = root.join(".credentials.json");
        std::fs::write(
            &credentials,
            br#"{"claudeAiOauth":{"accessToken":"test"}}"#,
        )
        .unwrap();

        assert!(!crate::constants::oauth::OAUTH_CREDENTIAL_SIDE_EFFECTS_ENABLED);
        let outcome = super::perform_logout_status(false).unwrap();
        assert!(outcome.removed_account_credentials);
        assert!(!credentials.exists());
        let _ = std::fs::remove_dir_all(root);
    }
}

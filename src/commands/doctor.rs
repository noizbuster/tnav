use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::auth::provider::normalized_callback_bind_host;
use crate::cli::GlobalArgs;
use crate::config::{
    AuthMethod, Config, ConfigError, ProfileConfig, load_from_path, resolve_config_path,
};
use crate::output::Output;
use crate::secrets::{KeyringSecretStore, SecretKind, SecretStore};
use crate::util::can_bind_localhost;
use crate::util::{can_open_browser, redact::redact_secret};

use super::{map_config_error, map_secret_store_error};
use crate::errors::TnavError;

#[derive(Debug, Serialize)]
struct DoctorReport {
    command: &'static str,
    config_path: String,
    active_profile: Option<String>,
    checks: Vec<DoctorCheck>,
}

#[derive(Debug, Serialize)]
struct DoctorCheck {
    name: &'static str,
    ok: bool,
    status: &'static str,
    details: String,
}

pub async fn run(global: &GlobalArgs) -> Result<(), TnavError> {
    let output = Output::new(global);
    let config_path = resolve_config_path(global.config.as_deref()).map_err(map_config_error)?;
    let mut checks = Vec::new();

    let config = match load_from_path(&config_path) {
        Ok(config) => {
            checks.push(DoctorCheck {
                name: "config_exists",
                ok: true,
                status: "ok",
                details: format!("found config at {}", config_path.display()),
            });
            Some(config)
        }
        Err(ConfigError::NotFound { .. }) => {
            checks.push(DoctorCheck {
                name: "config_exists",
                ok: false,
                status: "missing",
                details: format!("no config file at {}", config_path.display()),
            });
            None
        }
        Err(error) => {
            checks.push(DoctorCheck {
                name: "config_exists",
                ok: false,
                status: "invalid",
                details: error.to_string(),
            });
            None
        }
    };

    let store = KeyringSecretStore::new();
    let test_profile = "__doctor_test__";
    let test_value = "tnav-doctor-verification";
    let keyring_available = match store.save_secret(test_profile, SecretKind::ApiKey, test_value) {
        Ok(()) => {
            let load_result = store.load_secret(test_profile, SecretKind::ApiKey);
            let _ = store.delete_secret(test_profile, SecretKind::ApiKey);

            match load_result {
                Ok(Some(loaded)) if loaded == test_value => {
                    checks.push(DoctorCheck {
                        name: "keyring_available",
                        ok: true,
                        status: "ok",
                        details: "secure secret storage is reachable and functional".to_owned(),
                    });
                    true
                }
                Ok(Some(_)) => {
                    checks.push(DoctorCheck {
                        name: "keyring_available",
                        ok: false,
                        status: "corrupted",
                        details: "keyring returned incorrect value - save/load mismatch".to_owned(),
                    });
                    false
                }
                Ok(None) => {
                    checks.push(DoctorCheck {
                        name: "keyring_available",
                        ok: false,
                        status: "broken",
                        details: "keyring save succeeded but load returned nothing - secret service may not be running".to_owned(),
                    });
                    false
                }
                Err(error) => {
                    checks.push(DoctorCheck {
                        name: "keyring_available",
                        ok: false,
                        status: "load_failed",
                        details: format!("keyring save succeeded but load failed: {}", error),
                    });
                    false
                }
            }
        }
        Err(error) => {
            checks.push(DoctorCheck {
                name: "keyring_available",
                ok: false,
                status: "unavailable",
                details: map_secret_store_error(error).to_string(),
            });
            false
        }
    };

    checks.push(DoctorCheck {
        name: "browser_capability",
        ok: can_open_browser(),
        status: if can_open_browser() { "ok" } else { "missing" },
        details: if can_open_browser() {
            "found a browser opener on this system".to_owned()
        } else {
            "no supported browser opener command found on PATH".to_owned()
        },
    });

    let selected_profile_name = selected_profile_name(config.as_ref(), global.profile.as_deref());
    let bind_host = selected_profile_name
        .as_deref()
        .and_then(|name| config.as_ref().and_then(|config| config.profile(name)))
        .map(bind_safe_redirect_host)
        .unwrap_or("127.0.0.1");

    match can_bind_localhost(bind_host) {
        Ok(()) => checks.push(DoctorCheck {
            name: "localhost_bind",
            ok: true,
            status: "ok",
            details: format!("able to bind a loopback callback socket on {bind_host}"),
        }),
        Err(error) => checks.push(DoctorCheck {
            name: "localhost_bind",
            ok: false,
            status: "failed",
            details: format!("could not bind loopback callback socket on {bind_host}: {error}"),
        }),
    }

    append_profile_checks(
        &mut checks,
        config.as_ref(),
        selected_profile_name.as_deref(),
        &store,
        keyring_available,
    );

    let report = DoctorReport {
        command: "doctor",
        config_path: config_path.display().to_string(),
        active_profile: selected_profile_name,
        checks,
    };

    if output.is_json() {
        return output.print_json(&report);
    }

    for check in &report.checks {
        let marker = if check.ok { "[ok]" } else { "[warn]" };
        output.line(format!("{marker} {}: {}", check.name, check.details));
    }

    Ok(())
}

fn append_profile_checks(
    checks: &mut Vec<DoctorCheck>,
    config: Option<&Config>,
    selected_profile_name: Option<&str>,
    store: &KeyringSecretStore,
    keyring_available: bool,
) {
    let Some(config) = config else {
        checks.push(DoctorCheck {
            name: "active_profile_valid",
            ok: false,
            status: "skipped",
            details: "cannot validate the active profile until config loads successfully"
                .to_owned(),
        });
        return;
    };

    let Some(profile_name) = selected_profile_name else {
        checks.push(DoctorCheck {
            name: "active_profile_valid",
            ok: false,
            status: "missing",
            details: "no active profile is configured and no --profile was provided".to_owned(),
        });
        return;
    };

    let Some(profile) = config.profile(profile_name) else {
        checks.push(DoctorCheck {
            name: "active_profile_valid",
            ok: false,
            status: "invalid",
            details: format!("profile '{profile_name}' does not exist in config"),
        });
        return;
    };

    checks.push(DoctorCheck {
        name: "active_profile_valid",
        ok: true,
        status: "ok",
        details: format!(
            "profile '{profile_name}' resolves to provider '{}'",
            profile.provider
        ),
    });

    if matches!(profile.auth_method, AuthMethod::OAuth) {
        append_oauth_token_check(checks, profile_name, profile, store, keyring_available);
    } else {
        append_api_key_check(checks, profile_name, store, keyring_available);
    }
}

fn append_oauth_token_check(
    checks: &mut Vec<DoctorCheck>,
    profile_name: &str,
    profile: &ProfileConfig,
    store: &KeyringSecretStore,
    keyring_available: bool,
) {
    if !keyring_available {
        checks.push(DoctorCheck {
            name: "oauth_token_state",
            ok: false,
            status: "unavailable",
            details: format!(
                "cannot inspect OAuth tokens for profile '{profile_name}' because keyring is unavailable"
            ),
        });
        return;
    }

    if profile.client_id.is_none()
        || profile.authorization_url.is_none()
        || profile.token_url.is_none()
    {
        checks.push(DoctorCheck {
            name: "oauth_token_state",
            ok: false,
            status: "invalid",
            details: format!(
                "profile '{profile_name}' is missing OAuth provider configuration required for login"
            ),
        });
        return;
    }

    let access_token = store
        .load_secret(profile_name, SecretKind::OAuthAccessToken)
        .ok()
        .flatten();
    let metadata = store
        .load_secret(profile_name, SecretKind::OAuthMetadata)
        .ok()
        .flatten();
    match (access_token, metadata) {
        (Some(_), Some(metadata)) => {
            let parsed = serde_json::from_str::<crate::auth::StoredTokenMetadata>(&metadata);
            match parsed {
                Ok(metadata) => {
                    let now = current_unix_timestamp();
                    let expired = metadata
                        .expires_at_unix_seconds
                        .map(|expires_at| expires_at <= now)
                        .unwrap_or(false);
                    let status = if expired { "expired" } else { "present" };
                    let details = metadata
                        .expires_at_unix_seconds
                        .map(|expires_at| {
                            format!(
                                "OAuth token for profile '{profile_name}' is {status} (expires_at_unix_seconds={expires_at})"
                            )
                        })
                        .unwrap_or_else(|| {
                            format!(
                                "OAuth token for profile '{profile_name}' is present without an expiry timestamp"
                            )
                        });
                    checks.push(DoctorCheck {
                        name: "oauth_token_state",
                        ok: !expired,
                        status,
                        details,
                    });
                }
                Err(error) => checks.push(DoctorCheck {
                    name: "oauth_token_state",
                    ok: false,
                    status: "invalid",
                    details: format!(
                        "OAuth metadata for profile '{profile_name}' is unreadable: {error}"
                    ),
                }),
            }
        }
        _ => checks.push(DoctorCheck {
            name: "oauth_token_state",
            ok: false,
            status: "missing",
            details: format!("no stored OAuth token found for profile '{profile_name}'"),
        }),
    }
}

fn append_api_key_check(
    checks: &mut Vec<DoctorCheck>,
    profile_name: &str,
    store: &KeyringSecretStore,
    keyring_available: bool,
) {
    if !keyring_available {
        checks.push(DoctorCheck {
            name: "api_key_state",
            ok: false,
            status: "unavailable",
            details: format!(
                "cannot inspect API key for profile '{profile_name}' because keyring is unavailable"
            ),
        });
        return;
    }

    match store.load_secret(profile_name, SecretKind::ApiKey) {
        Ok(Some(api_key)) => checks.push(DoctorCheck {
            name: "api_key_state",
            ok: true,
            status: "present",
            details: format!(
                "API key is present for profile '{profile_name}' as {}",
                redact_secret(&api_key)
            ),
        }),
        Ok(None) => checks.push(DoctorCheck {
            name: "api_key_state",
            ok: false,
            status: "missing",
            details: format!("no stored API key found for profile '{profile_name}'"),
        }),
        Err(error) => checks.push(DoctorCheck {
            name: "api_key_state",
            ok: false,
            status: "error",
            details: error.to_string(),
        }),
    }
}

fn selected_profile_name(
    config: Option<&Config>,
    requested_profile: Option<&str>,
) -> Option<String> {
    if let Some(requested_profile) = requested_profile {
        return Some(requested_profile.to_owned());
    }

    config
        .and_then(Config::active_profile_name)
        .map(str::to_owned)
}

fn bind_safe_redirect_host(profile: &ProfileConfig) -> &str {
    normalized_callback_bind_host(&profile.redirect_host)
}

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

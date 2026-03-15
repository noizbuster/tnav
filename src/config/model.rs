use std::collections::HashMap;

use super::ConfigError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_profile: Option<String>,
    #[serde(default)]
    pub profiles: HashMap<String, ProfileConfig>,
}

impl Config {
    pub fn active_profile_name(&self) -> Option<&str> {
        self.active_profile.as_deref()
    }

    pub fn active_profile(&self) -> Option<(&str, &ProfileConfig)> {
        let name = self.active_profile_name()?;
        let profile = self.profiles.get(name)?;
        Some((name, profile))
    }

    pub fn profile(&self, name: &str) -> Option<&ProfileConfig> {
        self.profiles.get(name)
    }

    pub fn profile_mut(&mut self, name: &str) -> Option<&mut ProfileConfig> {
        self.profiles.get_mut(name)
    }

    pub fn set_active_profile(&mut self, profile: impl Into<String>) {
        self.active_profile = Some(profile.into());
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if let Some(active_profile) = self.active_profile_name() {
            if !self.profiles.contains_key(active_profile) {
                return Err(ConfigError::InvalidActiveProfile {
                    profile: active_profile.to_owned(),
                });
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileConfig {
    pub provider: String,
    pub auth_method: AuthMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revocation_url: Option<String>,
    #[serde(default = "default_redirect_host")]
    pub redirect_host: String,
    #[serde(default = "default_redirect_path")]
    pub redirect_path: String,
    #[serde(default)]
    pub ui: UiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthMethod {
    #[serde(rename = "oauth")]
    OAuth,
    #[serde(rename = "api_key")]
    ApiKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UiConfig {
    #[serde(default = "default_open_browser_automatically")]
    pub open_browser_automatically: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            open_browser_automatically: default_open_browser_automatically(),
        }
    }
}

fn default_redirect_host() -> String {
    "127.0.0.1".to_owned()
}

fn default_redirect_path() -> String {
    "/oauth/callback".to_owned()
}

fn default_open_browser_automatically() -> bool {
    true
}

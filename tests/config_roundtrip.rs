use std::collections::HashMap;

use tempfile::tempdir;
use tnav::config::{AuthMethod, Config, ProfileConfig, UiConfig, load_from_path, save_to_path};

#[test]
fn config_round_trips_through_toml_file_io() {
    let temp_dir = tempdir().expect("temp dir creates");
    let path = temp_dir.path().join("config.toml");

    let mut profiles = HashMap::new();
    profiles.insert(
        "default".to_owned(),
        ProfileConfig {
            provider: "GitHub".to_owned(),
            auth_method: AuthMethod::OAuth,
            base_url: Some("https://api.github.com".to_owned()),
            default_scopes: vec!["repo".to_owned(), "read:user".to_owned()],
            client_id: Some("client-id-123".to_owned()),
            authorization_url: Some("https://github.com/login/oauth/authorize".to_owned()),
            token_url: Some("https://github.com/login/oauth/access_token".to_owned()),
            revocation_url: Some("https://github.com/settings/connections/applications".to_owned()),
            redirect_host: "127.0.0.1".to_owned(),
            redirect_path: "/oauth/callback".to_owned(),
            ui: UiConfig {
                open_browser_automatically: false,
            },
        },
    );
    profiles.insert(
        "secondary".to_owned(),
        ProfileConfig {
            provider: "OpenAI".to_owned(),
            auth_method: AuthMethod::ApiKey,
            base_url: Some("https://api.openai.com".to_owned()),
            default_scopes: Vec::new(),
            client_id: None,
            authorization_url: None,
            token_url: None,
            revocation_url: None,
            redirect_host: "127.0.0.1".to_owned(),
            redirect_path: "/oauth/callback".to_owned(),
            ui: UiConfig::default(),
        },
    );

    let config = Config {
        active_profile: Some("default".to_owned()),
        profiles,
    };

    save_to_path(&config, &path).expect("config saves");
    let loaded = load_from_path(&path).expect("config loads");

    assert_eq!(loaded, config);
}

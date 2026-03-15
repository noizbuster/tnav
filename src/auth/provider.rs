use crate::auth::{AuthError, AuthResult};
use std::net::IpAddr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthProvider {
    pub name: String,
    pub client_id: String,
    pub authorization_url: String,
    pub token_url: String,
    pub revocation_url: Option<String>,
    pub scopes: Vec<String>,
    pub redirect_host: String,
    pub redirect_path: String,
}

impl OAuthProvider {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: impl Into<String>,
        client_id: impl Into<String>,
        authorization_url: impl Into<String>,
        token_url: impl Into<String>,
        revocation_url: Option<String>,
        scopes: Vec<String>,
        redirect_host: impl Into<String>,
        redirect_path: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            client_id: client_id.into(),
            authorization_url: authorization_url.into(),
            token_url: token_url.into(),
            revocation_url,
            scopes,
            redirect_host: redirect_host.into(),
            redirect_path: redirect_path.into(),
        }
    }

    pub fn validate(&self) -> AuthResult<()> {
        if self.name.trim().is_empty() {
            return Err(AuthError::InvalidProviderConfig {
                message: "provider name cannot be empty".to_owned(),
            });
        }

        if self.client_id.trim().is_empty() {
            return Err(AuthError::InvalidProviderConfig {
                message: format!("provider '{}' is missing a client_id", self.name),
            });
        }

        if self.redirect_host.trim().is_empty() {
            return Err(AuthError::InvalidProviderConfig {
                message: format!("provider '{}' is missing a redirect_host", self.name),
            });
        }

        if !is_loopback_redirect_host(&self.redirect_host) {
            return Err(AuthError::InvalidProviderConfig {
                message: format!("provider '{}' must use a loopback redirect_host", self.name),
            });
        }

        if !self.redirect_path.starts_with('/') {
            return Err(AuthError::InvalidProviderConfig {
                message: format!(
                    "provider '{}' must use an absolute redirect path",
                    self.name
                ),
            });
        }

        if self.authorization_url.trim().is_empty() || self.token_url.trim().is_empty() {
            return Err(AuthError::InvalidProviderConfig {
                message: format!(
                    "provider '{}' requires both authorization_url and token_url",
                    self.name
                ),
            });
        }

        Ok(())
    }

    pub fn redirect_uri_for_port(&self, port: u16) -> String {
        format!(
            "http://{}:{}{}",
            self.redirect_uri_host(),
            port,
            self.redirect_path
        )
    }

    pub fn callback_bind_host(&self) -> &str {
        normalized_loopback_host(&self.redirect_host)
    }

    pub fn redirect_uri_host(&self) -> String {
        let host = self.callback_bind_host();

        if is_ipv6_literal(host) {
            format!("[{host}]")
        } else {
            host.to_owned()
        }
    }
}

pub fn normalized_callback_bind_host(host: &str) -> &str {
    normalized_loopback_host(host)
}

fn is_loopback_redirect_host(host: &str) -> bool {
    let normalized_host = normalized_loopback_host(host);

    if normalized_host.eq_ignore_ascii_case("localhost") {
        return true;
    }

    normalized_host
        .parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

fn normalized_loopback_host(host: &str) -> &str {
    host.trim().trim_matches(['[', ']'])
}

fn is_ipv6_literal(host: &str) -> bool {
    host.parse::<IpAddr>()
        .map(|ip| matches!(ip, IpAddr::V6(_)))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::OAuthProvider;

    #[test]
    fn redirect_uri_uses_host_port_and_path() {
        let provider = OAuthProvider::new(
            "example",
            "client-id",
            "https://example.com/oauth/authorize",
            "https://example.com/oauth/token",
            None,
            vec!["read".to_owned()],
            "127.0.0.1",
            "/oauth/callback",
        );

        assert_eq!(
            provider.redirect_uri_for_port(43123),
            "http://127.0.0.1:43123/oauth/callback"
        );
    }

    #[test]
    fn redirect_uri_brackets_ipv6_loopback_host() {
        let provider = OAuthProvider::new(
            "example",
            "client-id",
            "https://example.com/oauth/authorize",
            "https://example.com/oauth/token",
            None,
            vec!["read".to_owned()],
            "::1",
            "/oauth/callback",
        );

        assert_eq!(
            provider.redirect_uri_for_port(43123),
            "http://[::1]:43123/oauth/callback"
        );
    }

    #[test]
    fn callback_bind_host_normalizes_bracketed_ipv6_loopback() {
        let provider = OAuthProvider::new(
            "example",
            "client-id",
            "https://example.com/oauth/authorize",
            "https://example.com/oauth/token",
            None,
            vec!["read".to_owned()],
            "[::1]",
            "/oauth/callback",
        );

        assert_eq!(provider.callback_bind_host(), "::1");
        assert_eq!(provider.redirect_uri_host(), "[::1]");
    }

    #[test]
    fn validate_accepts_loopback_hosts() {
        for host in ["127.0.0.1", "localhost", "::1", "[::1]"] {
            let provider = OAuthProvider::new(
                "example",
                "client-id",
                "https://example.com/oauth/authorize",
                "https://example.com/oauth/token",
                None,
                vec!["read".to_owned()],
                host,
                "/oauth/callback",
            );

            assert!(
                provider.validate().is_ok(),
                "expected host '{host}' to pass"
            );
        }
    }

    #[test]
    fn validate_rejects_non_loopback_host() {
        let provider = OAuthProvider::new(
            "example",
            "client-id",
            "https://example.com/oauth/authorize",
            "https://example.com/oauth/token",
            None,
            vec!["read".to_owned()],
            "192.168.1.10",
            "/oauth/callback",
        );

        assert!(provider.validate().is_err());
    }
}

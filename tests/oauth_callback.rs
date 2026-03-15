use std::time::Duration;

use tnav::auth::{AwaitCallbackResult, OAuthProvider, OAuthService};

fn example_provider() -> OAuthProvider {
    OAuthProvider::new(
        "example",
        "client-id",
        "https://example.com/oauth/authorize",
        "https://example.com/oauth/token",
        None,
        vec!["read".to_owned()],
        "127.0.0.1",
        "/oauth/callback",
    )
}

#[tokio::test]
async fn oauth_service_receives_local_callback_code() {
    let service = OAuthService::new().expect("service builds");
    let provider = example_provider();
    let callback_server = service
        .start_callback_server(&provider)
        .await
        .expect("callback server binds");
    let redirect_uri = provider.redirect_uri_for_port(callback_server.local_addr().port());
    let expected_state = "state-123";
    let callback_url = format!("{redirect_uri}?code=auth-code-123&state={expected_state}");

    let callback_task = tokio::spawn(async move {
        let response = reqwest::get(callback_url)
            .await
            .expect("callback request succeeds");
        let status = response.status();
        let body = response.text().await.expect("callback body reads");
        (status, body)
    });

    let callback_result = service
        .await_callback_with_timeout(callback_server, expected_state, Duration::from_secs(2))
        .await
        .expect("callback result received");

    let (status, body) = callback_task.await.expect("callback task joins");

    assert!(status.is_success());
    assert!(body.contains("Login completed"));
    assert_eq!(
        callback_result,
        AwaitCallbackResult::AuthorizationCode {
            code: "auth-code-123".to_owned(),
            state: expected_state.to_owned(),
        }
    );
}

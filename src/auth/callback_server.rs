use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinHandle;

use crate::auth::{AuthError, AuthResult};

const CALLBACK_SUCCESS_PAGE: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>tnav login complete</title></head><body><main><h1>Login completed</h1><p>You can safely close this tab.</p></main></body></html>";
const CALLBACK_FAILURE_PAGE: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>tnav login not completed</title></head><body><main><h1>Login not completed</h1><p>Return to the terminal for details.</p></main></body></html>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallbackPayload {
    AuthorizationCode {
        code: String,
        state: String,
    },
    ProviderError {
        error: String,
        description: Option<String>,
        state: Option<String>,
    },
    InvalidRequest {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallbackWaitResult {
    Received(CallbackPayload),
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthCallbackError {
    pub error: String,
    pub description: Option<String>,
}

pub struct CallbackServerHandle {
    local_addr: SocketAddr,
    redirect_uri: String,
    callback_rx: oneshot::Receiver<CallbackPayload>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    server_task: JoinHandle<Result<(), String>>,
}

impl CallbackServerHandle {
    pub async fn bind(host: &str, redirect_path: &str) -> AuthResult<Self> {
        let listener =
            TcpListener::bind((host, 0))
                .await
                .map_err(|error| AuthError::CallbackBindFailed {
                    host: host.to_owned(),
                    message: error.to_string(),
                })?;

        let local_addr = listener
            .local_addr()
            .map_err(|error| AuthError::CallbackBindFailed {
                host: host.to_owned(),
                message: error.to_string(),
            })?;
        let redirect_uri = format!("http://{}:{}{}", host, local_addr.port(), redirect_path);

        let (callback_tx, callback_rx) = oneshot::channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let app = Router::new()
            .route(redirect_path, get(handle_callback))
            .with_state(CallbackState::new(callback_tx));

        let server_task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await
                .map_err(|error| error.to_string())
        });

        Ok(Self {
            local_addr,
            redirect_uri,
            callback_rx,
            shutdown_tx: Some(shutdown_tx),
            server_task,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    pub async fn wait_for_callback(mut self, timeout: Duration) -> AuthResult<CallbackWaitResult> {
        let receive_result = tokio::time::timeout(timeout, &mut self.callback_rx).await;

        let wait_result = match receive_result {
            Ok(Ok(payload)) => CallbackWaitResult::Received(payload),
            Ok(Err(_)) => return Err(AuthError::CallbackChannelClosed),
            Err(_) => CallbackWaitResult::TimedOut,
        };

        self.shutdown().await?;

        Ok(wait_result)
    }

    async fn shutdown(&mut self) -> AuthResult<()> {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        match (&mut self.server_task).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(message)) => Err(AuthError::CallbackServerFailed { message }),
            Err(error) => Err(AuthError::CallbackServerFailed {
                message: error.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone)]
struct CallbackState {
    sender: Arc<Mutex<Option<oneshot::Sender<CallbackPayload>>>>,
}

impl CallbackState {
    fn new(sender: oneshot::Sender<CallbackPayload>) -> Self {
        Self {
            sender: Arc::new(Mutex::new(Some(sender))),
        }
    }

    async fn send_once(&self, payload: CallbackPayload) {
        let mut sender = self.sender.lock().await;

        if let Some(sender) = sender.take() {
            let _ = sender.send(payload);
        }
    }
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

async fn handle_callback(
    State(state): State<CallbackState>,
    Query(query): Query<CallbackQuery>,
) -> impl IntoResponse {
    let (status, payload, html) = if let Some(error) = query.error {
        (
            StatusCode::BAD_REQUEST,
            CallbackPayload::ProviderError {
                error,
                description: query.error_description,
                state: query.state,
            },
            CALLBACK_FAILURE_PAGE,
        )
    } else if let (Some(code), Some(state_value)) = (query.code, query.state) {
        (
            StatusCode::OK,
            CallbackPayload::AuthorizationCode {
                code,
                state: state_value,
            },
            CALLBACK_SUCCESS_PAGE,
        )
    } else {
        (
            StatusCode::BAD_REQUEST,
            CallbackPayload::InvalidRequest {
                message: "callback query must include either code+state or error".to_owned(),
            },
            CALLBACK_FAILURE_PAGE,
        )
    };

    state.send_once(payload).await;

    (status, Html(html))
}

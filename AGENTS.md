# PROJECT KNOWLEDGE BASE

**Generated:** 2026-03-16
**Commit:** a2f779d
**Branch:** @feat/more-providers

## OVERVIEW

`tnav` is a Rust CLI (v0.1.6) that generates shell commands from natural-language prompts via LLM providers. Also handles OAuth/API-key authentication flows with system keyring storage. Uses clap for CLI, tokio async runtime, inquire for interactive prompts.

## STRUCTURE

```
tnav/
├── src/
│   ├── llm/        # LLM provider abstraction (Ollama, OpenAI, OpenAI-compatible)
│   ├── auth/       # OAuth PKCE flow, callback server, browser opener
│   ├── commands/   # CLI command handlers (init, connect, model, ask, doctor)
│   ├── config/     # TOML config load/save, path resolution
│   ├── ui/         # Interactive prompts via inquire
│   ├── secrets/    # Keyring secret storage trait + memory impl
│   └── profiles/   # (placeholder)
├── tests/          # Integration tests
└── Cargo.toml
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Add new CLI subcommand | `src/cli.rs` + `src/commands/` | Add to `Command` enum, dispatch in `app.rs` |
| Add LLM provider | `src/llm/` | Implement `LlmProvider` trait |
| Modify OAuth flow | `src/auth/oauth.rs`, `src/auth/callback_server.rs` | PKCE in `pkce.rs` |
| Change config format | `src/config/model.rs` | Separate `config.toml` (profiles) and `llm.toml` (providers) |
| Add interactive prompt | `src/ui/prompts.rs` | Uses inquire crate |
| Secret storage | `src/secrets/mod.rs` | `SecretStore` trait, `KeyringSecretStore` impl |
| Error handling | `src/errors.rs` | Central `TnavError` enum with exit codes |

## CODE MAP

| Symbol | Type | Location | Role |
|--------|------|----------|------|
| `Cli` | Struct | `src/cli.rs:10` | CLI args parser (clap derive) |
| `Command` | Enum | `src/cli.rs:27` | All subcommands |
| `run` | Fn | `src/app.rs:6` | Command dispatch |
| `LlmProvider` | Trait | `src/llm/provider.rs:49` | LLM abstraction |
| `StreamSink` | Trait | `src/llm/provider.rs:45` | Streaming callback |
| `SecretStore` | Trait | `src/secrets/mod.rs:11` | Keyring abstraction |
| `OAuthService` | Struct | `src/auth/oauth.rs` | PKCE OAuth flow |
| `TnavError` | Enum | `src/errors.rs:4` | Top-level error type |

## CONVENTIONS

- **Edition 2024** - uses latest Rust edition
- **Clap derive** - `#[derive(Parser, Subcommand)]` for CLI
- **Thiserror** - all error types use `#[derive(Error)]`
- **Tracing** - structured logging via `tracing` crate
- **Async** - tokio multi-threaded runtime, `async fn` for I/O
- **Tests** - unit tests in same file (`#[cfg(test)]`), integration in `tests/`
- **Visibility** - modules `pub`, internals `pub(crate)` or private

## ANTI-PATTERNS

None explicitly documented. Project follows standard Rust conventions.

## UNIQUE STYLES

- **Two-config split**: `config.toml` for profiles/auth, `llm.toml` for LLM providers
- **Inquire prompts**: all interactive UI goes through `src/ui/` wrapper
- **Env lock pattern**: tests use `Mutex` guard for env var isolation (see `tests/llm_integration.rs`)
- **Provider instance naming**: supports multiple instances of same provider type (e.g., `openai-compatible-2`)

## COMMANDS

```bash
# Development
cargo build
cargo run -- --help
cargo run -- doctor

# Verification (matches CI)
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo check --all-features

# Release
dist build --tag vX.Y.Z --target x86_64-unknown-linux-gnu --artifacts=all
```

## NOTES

- **Keyring dependency**: requires working system keyring on Linux (gnome-keyring/KWallet)
- **OAuth callback**: binds localhost socket, requires browser opener
- **Exit codes**: `ExitCode::from(2)` for user/config errors, `from(1)` for runtime failures
- **Streaming**: LLM responses stream to stdout when not in JSON/quiet mode
- **Legacy support**: `lmstudio` provider name auto-migrates to `openai-compatible`

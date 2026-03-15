# tnav Plan

## 1. Objective

Build `tnav` as a Rust-based interactive CLI for macOS and Linux that can be installed via GitHub Releases using a `curl | sh` installer flow.

The project should optimize for:
- excellent interactive terminal UX
- safe handling of API keys and OAuth tokens
- minimal external runtime dependencies
- single-binary distribution for macOS and Linux
- clean architecture that another AI can scaffold and extend

Repository target:
- GitHub owner: `noizbuster`
- Repository: `tnav`
- Binary name: `tnav`

---

## 2. Product assumptions

The exact domain behavior of `tnav` is not fully specified yet, so the initial boilerplate should focus on a strong platform foundation rather than business logic.

Assume `tnav` needs to support these interaction patterns:
- guided setup wizard
- single-choice menus
- multi-select checklists
- secret input for API keys and tokens
- browser-based OAuth login with local callback handling
- config inspection and editing
- non-interactive flags for CI or scripting later

The first milestone is **a production-grade CLI skeleton**, not a fully-featured end-user product.

---

## 3. Chosen stack

Use this stack as the default architectural baseline:

### Core crates
- `clap`
  - role: command-line entrypoint, subcommands, flags, help output
- `inquire`
  - role: interactive prompts, select, multi-select, text, password, confirm
- `webbrowser`
  - role: open OAuth authorization URLs in the user’s default browser
- `oauth2`
  - role: Authorization Code flow with PKCE for public/native CLI apps
- `axum`
  - role: lightweight localhost callback server for OAuth redirect handling
- `keyring`
  - role: store secrets in platform secure storage when available

### Supporting crates
Use these supporting crates unless there is a strong reason not to:
- `tokio` for async runtime
- `reqwest` with rustls TLS for HTTP requests
- `serde` + `serde_json` + `toml` for config serialization
- `thiserror` and/or `anyhow` for error handling
- `tracing` + `tracing-subscriber` for structured logs
- `url` for callback/query parsing when useful
- a config-path helper such as `directories` or equivalent

### Why this stack
- `clap` should own the command surface and non-interactive API.
- `inquire` should own the human-facing interactive flow.
- `oauth2` + PKCE is the safest standard baseline for a native CLI.
- `axum` is simpler than building a raw callback server by hand and keeps the OAuth flow easy to test.
- `keyring` avoids plain-text token storage by default.
- Rust gives portable binaries that are easy to ship via GitHub Releases.

---

## 4. Architecture principles

1. **Separate transport from UX**
   - Prompt rendering should not contain business logic.
   - OAuth/token code should not know about prompt details.

2. **Interactive-first, automation-friendly**
   - Every interactive flow should later be callable via flags.
   - Example: `tnav auth login` may prompt by default, but should accept `--provider`, `--client-id`, `--client-secret`, `--scopes`, etc.

3. **Secure by default**
   - Store secrets in keyring first.
   - Do not write access tokens to plain-text config files by default.
   - If file fallback is needed, require explicit opt-in and restrictive permissions.

4. **Testable modules**
   - Put most logic in library code.
   - Keep `main.rs` thin.

5. **Strong fallback behavior**
   - If browser launch fails, print URL for manual open.
   - If localhost callback fails, allow manual paste of auth code as fallback.
   - If keyring is unavailable, explain the issue clearly and offer a guarded fallback path.

---

## 5. High-level user journeys

### 5.1 First-run setup
Command:
- `tnav init`

Expected flow:
1. Show welcome banner.
2. Ask user whether to configure API key auth, OAuth auth, or both.
3. Ask provider/environment selection.
4. Ask optional defaults such as scopes, base URL, profile name, telemetry preference.
5. Save config and secrets.
6. Print next steps.

### 5.2 API key setup
Command:
- `tnav auth api-key`

Expected flow:
1. Select provider or profile.
2. Prompt for API key with secret input.
3. Validate non-empty and optional format constraints.
4. Save to keyring.
5. Optionally test connectivity.

### 5.3 OAuth login
Command:
- `tnav auth login`

Expected flow:
1. Load provider metadata from config.
2. Start localhost callback server on a random available loopback port.
3. Generate PKCE verifier/challenge and CSRF state.
4. Build authorization URL.
5. Open browser.
6. Receive callback with code.
7. Verify state.
8. Exchange code for token.
9. Save token/refresh token in keyring.
10. Render success message.

### 5.4 OAuth logout
Command:
- `tnav auth logout`

Expected flow:
1. Select profile.
2. Remove stored secrets from keyring.
3. Optionally revoke token if revocation endpoint exists.
4. Preserve non-secret config.

### 5.5 Config management
Commands:
- `tnav config show`
- `tnav config set`
- `tnav config path`
- `tnav profile list`
- `tnav profile use`

Expected behavior:
- Show only non-secret settings by default.
- Mask secret-related fields if they are displayed at all.

### 5.6 Diagnostics
Command:
- `tnav doctor`

Expected checks:
- config file existence
- keyring availability
- browser-open capability
- localhost bind capability
- active profile validity
- token presence / expiry state

---

## 6. Proposed command surface

This is a starter command design. Another AI may adjust names slightly, but should keep the overall intent.

```text
tnav
├─ init
├─ auth
│  ├─ login
│  ├─ logout
│  ├─ api-key
│  ├─ status
│  └─ revoke
├─ config
│  ├─ show
│  ├─ set
│  ├─ path
│  └─ reset
├─ profile
│  ├─ list
│  ├─ add
│  ├─ remove
│  └─ use
├─ doctor
└─ version
```

### Global flags
Add these from the beginning:
- `--profile <name>`
- `--config <path>`
- `--verbose`
- `--quiet`
- `--no-browser`
- `--json`
- `--yes`
- `--non-interactive`

### Command behavior guidelines
- Without `--non-interactive`, commands may prompt.
- With `--non-interactive`, missing required data should fail clearly.
- `--json` should make machine-readable output possible for status-type commands.

---

## 7. Project layout

Use a library-first layout.

```text
tnav/
├─ Cargo.toml
├─ README.md
├─ LICENSE
├─ .gitignore
├─ .github/
│  └─ workflows/
│     ├─ ci.yml
│     └─ release.yml
├─ dist-workspace.toml            # optional if using cargo-dist split config
├─ src/
│  ├─ main.rs
│  ├─ lib.rs
│  ├─ cli.rs
│  ├─ app.rs
│  ├─ errors.rs
│  ├─ output.rs
│  ├─ commands/
│  │  ├─ mod.rs
│  │  ├─ init.rs
│  │  ├─ auth.rs
│  │  ├─ config.rs
│  │  ├─ profile.rs
│  │  └─ doctor.rs
│  ├─ ui/
│  │  ├─ mod.rs
│  │  ├─ prompts.rs
│  │  ├─ theme.rs
│  │  └─ wizard.rs
│  ├─ auth/
│  │  ├─ mod.rs
│  │  ├─ oauth.rs
│  │  ├─ callback_server.rs
│  │  ├─ browser.rs
│  │  ├─ pkce.rs
│  │  ├─ tokens.rs
│  │  └─ provider.rs
│  ├─ config/
│  │  ├─ mod.rs
│  │  ├─ model.rs
│  │  ├─ load.rs
│  │  ├─ save.rs
│  │  └─ paths.rs
│  ├─ secrets/
│  │  ├─ mod.rs
│  │  ├─ keyring_store.rs
│  │  └─ fallback_store.rs
│  ├─ profiles/
│  │  ├─ mod.rs
│  │  └─ selection.rs
│  └─ util/
│     ├─ mod.rs
│     ├─ fs.rs
│     ├─ net.rs
│     └─ redact.rs
├─ tests/
│  ├─ cli_smoke.rs
│  ├─ config_roundtrip.rs
│  ├─ oauth_flow.rs
│  └─ doctor.rs
└─ fixtures/
   ├─ sample-config.toml
   └─ sample-oauth-response.json
```

---

## 8. Responsibility of each module

### `main.rs`
- initialize tracing
- call async app runner
- map top-level error to exit code

### `lib.rs`
- expose `run()` entrypoint
- keep integration tests easy

### `cli.rs`
- define `clap` parser structs
- define commands and global flags
- perform only lightweight parsing/validation

### `app.rs`
- central command dispatch
- translate parsed CLI inputs into command handlers

### `commands/*`
- one file per logical command group
- orchestrate prompts + services + output
- avoid low-level persistence and HTTP details

### `ui/prompts.rs`
- reusable wrappers around `inquire`
- provide consistent labels, help text, validation, cancel handling
- support reusable prompt helpers:
  - `prompt_profile_name()`
  - `prompt_api_key()`
  - `select_provider()`
  - `multiselect_scopes()`
  - `confirm_overwrite()`

### `ui/wizard.rs`
- multi-step guided flows such as `tnav init`
- keep step transitions explicit and testable

### `auth/oauth.rs`
- build auth URL
- create PKCE challenge/verifier
- verify returned state
- exchange auth code for token
- optionally refresh/revoke token

### `auth/callback_server.rs`
- start loopback server
- wait for single callback
- shut down immediately after success/error/timeout
- expose clean async API back to `auth/oauth.rs`

### `auth/browser.rs`
- wrapper around `webbrowser`
- fallback to printed URL if auto-open fails

### `auth/provider.rs`
- provider configuration model
- auth URL, token URL, revocation URL, scopes, redirect path
- keep this independent of prompt layer

### `config/model.rs`
- typed representation of config file
- store non-secret data only

### `config/load.rs` and `save.rs`
- config discovery, read, parse, validate, persist
- preserve formatting simplicity over fancy comments

### `config/paths.rs`
- central place for config/cache/log path decisions
- avoid path logic duplicated across commands

### `secrets/keyring_store.rs`
- save/load/delete API keys and OAuth tokens
- naming convention for service/account keys

### `secrets/fallback_store.rs`
- optional, disabled by default
- only used with explicit opt-in when keyring is unavailable
- must use restrictive file permissions and clear warnings

### `output.rs`
- plain text vs JSON output helpers
- redaction helpers for secrets

### `util/redact.rs`
- reusable secret masking
- examples:
  - `sk-abc...xyz`
  - `token stored in keyring`

---

## 9. Data model

## 9.1 Config file
Store this in a user config directory, for example:
- macOS: standard user config location
- Linux: XDG config location

Example file: `config.toml`

```toml
active_profile = "default"

[profiles.default]
provider = "example"
auth_method = "oauth"
base_url = "https://api.example.com"
default_scopes = ["read", "write"]
client_id = "your-public-client-id"
redirect_host = "127.0.0.1"
redirect_path = "/oauth/callback"

[profiles.default.ui]
open_browser_automatically = true
```

Rules:
- never store access tokens here by default
- never store API keys here by default
- only store public metadata and preferences

## 9.2 Secret storage model
Store secrets separately in keyring.

Suggested key naming:
- service: `tnav`
- account examples:
  - `profile/default/api_key`
  - `profile/default/oauth_access_token`
  - `profile/default/oauth_refresh_token`
  - `profile/default/oauth_metadata`

Secret metadata may include:
- issued_at
- expires_at
- token_type
- scope string

If metadata is not sensitive, it may be stored in config instead, but keeping it alongside the secret is simpler for the first version.

---

## 10. OAuth flow design

Use **Authorization Code + PKCE**.

### 10.1 Flow details
1. Resolve active profile and provider config.
2. Bind localhost on `127.0.0.1:0` to get an available port.
3. Build redirect URI using assigned port and configured callback path.
4. Generate:
   - CSRF state
   - PKCE challenge/verifier
5. Start callback server.
6. Build authorization URL.
7. Open browser via `webbrowser`.
8. Wait for exactly one callback.
9. Extract `code` and `state` from query params.
10. Compare returned state with original CSRF state.
11. Exchange code for token using `oauth2`.
12. Save token set in keyring.
13. Return success page to browser.
14. Shut down callback server.

### 10.2 Timeout behavior
- default timeout: 180 seconds
- if timeout occurs:
  - stop server
  - print manual recovery steps
  - optionally offer manual code entry flow

### 10.3 Fallback modes
Support these fallbacks from the beginning:
- browser open fails -> print URL and ask user to open manually
- local callback bind fails -> offer device/manual mode placeholder or manual pasted-code mode
- callback never arrives -> allow pasted code and state verification if provider supports it

### 10.4 Success HTML page
Serve a minimal HTML page on callback completion:
- message: login completed
- safe to close this tab
- no token data in HTML

### 10.5 Security requirements
- verify state every time
- use PKCE every time
- do not log raw tokens
- redact auth code and token values from traces
- shut down callback server immediately after completion

---

## 11. Interactive UX guidelines

### Prompt guidelines
- every prompt should have a clear title and optional help line
- destructive actions require confirmation
- secret input uses password-style prompt
- allow cancel with Ctrl+C and treat it as a clean user cancellation, not a panic

### Recommended prompt patterns
- provider selection: `Select`
- scopes selection: `MultiSelect`
- profile name: `Text` with validator
- API key: `Password` or secret prompt
- overwrite existing config: `Confirm`
- long values such as custom JSON or long text: `Editor` if needed later

### Paste behavior
For API keys or long secret strings:
- do not trim internal characters
- trim only surrounding whitespace/newlines as appropriate
- validate non-empty after normalization
- optionally offer a visible confirmation such as length or masked preview

### Multi-step wizard design
The `init` command should use a wizard pattern:
1. select auth type
2. select provider
3. gather provider-specific values
4. authenticate
5. validate
6. save
7. summary

Keep each step in its own function.

---

## 12. Config and secret handling rules

### Config file
Allowed:
- active profile
- provider name
- client ID
- base URL
- selected scopes
- UX preferences
- callback path/host

Disallowed by default:
- API keys
- access tokens
- refresh tokens
- client secret for public-native flows unless absolutely necessary

### File permissions
When writing config files:
- create parent directory if needed
- use restrictive permissions where platform permits
- avoid world-readable files for anything sensitive

### Fallback secret storage
Only implement if needed.

If implemented, it must:
- require explicit user consent or explicit flag
- store in a dedicated secrets file separate from config
- set restrictive permissions
- clearly warn that platform secure storage is preferred

---

## 13. Error model and exit codes

Define a typed error hierarchy.

Example categories:
- `UserCancelled`
- `ConfigNotFound`
- `ConfigInvalid`
- `SecretStoreUnavailable`
- `SecretStoreWriteFailed`
- `BrowserOpenFailed`
- `OAuthCallbackTimeout`
- `OAuthStateMismatch`
- `OAuthExchangeFailed`
- `NetworkError`
- `UnsupportedMode`

Exit code guidance:
- `0` success
- `1` generic failure
- `2` user cancelled / invalid usage / missing required input

CLI behavior:
- human-readable by default
- JSON-serializable status object for `--json`

---

## 14. Logging and observability

Use `tracing` with these rules:
- default log level: warn/info depending on command
- `--verbose` increases detail
- never log raw secrets
- use explicit redaction wrappers for token-like values
- callback and auth logs should include only safe metadata

Suggested log fields:
- profile name
- provider name
- redirect host/port
- timeout reached
- token expiry timestamp

Do not log:
- API keys
- auth codes
- access tokens
- refresh tokens
- client secret

---

## 15. Testing strategy

### 15.1 Unit tests
Cover:
- config parse/write round-trip
- profile selection logic
- secret redaction helpers
- prompt-independent validation functions
- redirect URI construction
- state verification

### 15.2 Integration tests
Cover:
- CLI help output
- `init --non-interactive` validation failures
- config path discovery
- mock OAuth callback flow
- doctor command behavior with mocked environment

### 15.3 Mocking strategy
- abstract secret store behind a trait
- abstract browser opener behind a trait
- abstract provider/token exchange behind a trait or test adapter
- run callback server tests against loopback only

### 15.4 Manual QA checklist
- macOS login flow succeeds
- Linux login flow succeeds
- API key paste works for long values
- MultiSelect UX is usable in a normal terminal
- Ctrl+C during prompt exits cleanly
- `curl | sh` install produces working binary on both platforms

---

## 16. Implementation phases

## Phase 0: Repository bootstrap
Deliverables:
- Rust project skeleton
- `clap` parser with command tree
- async runtime wiring
- error and logging foundation
- placeholder README

Acceptance criteria:
- `cargo run -- --help` works
- `cargo fmt`, `cargo clippy`, `cargo test` are wired in CI

## Phase 1: Config + profiles foundation
Deliverables:
- config path discovery
- typed config model
- load/save functions
- active profile support

Acceptance criteria:
- config round-trip tests pass
- `tnav config path` and `tnav config show` work

## Phase 2: Interactive prompt layer
Deliverables:
- reusable prompt helpers using `inquire`
- `tnav init` guided flow
- provider/profile selection prompts
- API key prompt path

Acceptance criteria:
- `tnav init` can create a usable profile without panicking
- prompt cancellation is handled gracefully

## Phase 3: Secret storage
Deliverables:
- keyring integration
- save/load/delete for API keys and tokens
- masked secret status output

Acceptance criteria:
- `tnav auth api-key` stores and reads key successfully
- tokens are not written to config file

## Phase 4: OAuth login
Deliverables:
- localhost callback server
- browser opener integration
- PKCE flow with `oauth2`
- token persistence

Acceptance criteria:
- successful login path works against a test provider or mock provider
- state mismatch is detected and surfaced correctly
- timeout path is handled cleanly

## Phase 5: Diagnostics + polish
Deliverables:
- `doctor` command
- improved error messages
- JSON output for selected commands
- README install and auth docs

Acceptance criteria:
- common setup problems are discoverable via `doctor`
- CI and release automation are green

## Phase 6: Distribution
Deliverables:
- GitHub Actions release workflow
- GitHub Releases artifacts for macOS and Linux
- shell installer from release assets
- documented `curl | sh` install path

Acceptance criteria:
- tagged release uploads platform binaries
- installer downloads and installs the correct asset

---

## 17. Boilerplate tasks for another AI

The next AI should perform these tasks in order.

### Task 1: scaffold the project
Create:
- `Cargo.toml`
- `src/main.rs`
- `src/lib.rs`
- `src/cli.rs`
- `src/app.rs`
- `src/errors.rs`
- module directories listed above

### Task 2: implement command parser
Implement the command tree from section 6 using `clap` derive macros.

### Task 3: implement config model
Create a TOML-backed config model with:
- `active_profile`
- `profiles: HashMap<String, ProfileConfig>`

### Task 4: implement prompt wrappers
Create a prompt service around `inquire` that can be mocked or swapped later.

### Task 5: implement secret store trait
Define a trait such as:
- `save_secret(profile, kind, value)`
- `load_secret(profile, kind)`
- `delete_secret(profile, kind)`

Provide:
- keyring-backed implementation
- in-memory test implementation

### Task 6: implement OAuth flow service
Create service methods for:
- `build_authorization_url`
- `start_callback_server`
- `await_callback`
- `exchange_code`
- `save_token_set`

### Task 7: implement `init`, `auth api-key`, `auth login`, `doctor`
These are the first real commands that should work end-to-end.

### Task 8: add tests
At minimum:
- config round-trip
- secret redaction
- CLI smoke tests
- mocked OAuth callback test

### Task 9: add release automation
Use GitHub Actions + `cargo-dist` for release packaging.

### Task 10: write README
Include:
- install via `curl | sh`
- local build instructions
- auth setup examples
- troubleshooting notes

---

## 18. Distribution and release plan

Use GitHub Releases as the distribution channel.

### Recommended packaging approach
Use `cargo-dist` to generate release artifacts and shell installer assets.

### Supported targets
Initial target matrix:
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`

### Release trigger
- tag pushes like `v0.1.0`

### Expected release outputs
- per-platform tarballs or archives
- checksums
- installer shell script asset

### Installer UX goal
Users should be able to install with a command in this shape:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/noizbuster/tnav/releases/latest/download/tnav-installer.sh | sh
```

If asset naming differs, the README must reflect the actual generated asset names.

### Local development commands
The next AI should wire at least these commands into documentation:

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo run -- --help
cargo run -- init
```

---

## 19. CI plan

Create two workflows.

### `ci.yml`
Run on push and pull request:
- format check
- clippy
- tests
- optional `cargo check --all-features`

### `release.yml`
Run on version tag push:
- build release artifacts
- publish GitHub Release
- publish shell installer asset

Keep CI strict enough that boilerplate stays healthy.

---

## 20. README requirements

The README should include:
- what `tnav` is
- supported platforms
- installation command
- local build instructions
- how config is stored
- how secrets are stored
- how OAuth login works at a high level
- troubleshooting for browser/keyring/callback issues

Suggested troubleshooting entries:
- browser did not open
- keyring unavailable on Linux session
- localhost callback blocked or port bind failure
- token revoked or expired

---

## 21. Non-goals for the initial scaffold

Do not overbuild these in v0:
- full-screen TUI
- plugin system
- device-code OAuth unless required by provider
- cross-shell completion polish
- advanced profile inheritance
- encrypted custom secrets vault unless keyring fallback becomes necessary

Focus on getting the interactive wizard, auth flows, secret handling, and release pipeline solid first.

---

## 22. Definition of done for the initial boilerplate

The initial boilerplate is complete when all of the following are true:

1. `tnav --help` shows a clean command tree.
2. `tnav init` creates a valid config interactively.
3. `tnav auth api-key` stores a secret in keyring.
4. `tnav auth login` can complete a localhost browser OAuth flow.
5. `tnav doctor` reports common environment issues.
6. Secrets are redacted from output and logs.
7. CI passes on every push.
8. A tagged GitHub release builds installable binaries.
9. Users can install from GitHub Releases with a `curl | sh` flow.

---

## 23. Implementation notes for the next AI

When generating code, the next AI should follow these rules:
- prefer clear modular code over clever abstractions
- keep public interfaces small
- use traits only where they improve testing or platform isolation
- keep secrets and config physically separate
- avoid embedding provider-specific logic into prompt code
- make command handlers small and composable
- avoid blocking operations inside async paths where possible
- return structured errors, then format for CLI at the boundary

The next AI should generate a working project skeleton first, then flesh out the flows iteratively.

---

## 24. Optional future enhancements

After the initial release, consider:
- OIDC support with `openidconnect` when ID token verification is required
- shell completions
- command aliases
- telemetry opt-in prompt
- provider templates and metadata discovery
- self-update command
- device-code flow for headless environments
- secure export/import of profile metadata


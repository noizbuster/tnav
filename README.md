<img src="banner.png" alt="tnav banner" width="100%">

# tnav

`tnav` is a Rust CLI that currently combines two working areas:

- interactive profile and authentication setup for API-key and OAuth-backed services
- LLM-backed shell command generation from natural-language prompts

The project is still early-stage, but the implemented flows are real: guided profile setup,
secure secret storage, browser-based OAuth login, environment diagnostics, LLM provider
management, model selection, and prompt-driven shell command execution.

## Implementation status

Implemented today:

- `init`
- `auth api-key`
- `auth login`
- `doctor`
- `connect`
- `model [MODEL]`
- bare prompt input such as `tnav show current directory`
- plain `tnav` interactive prompt mode

Command groups that parse but still return an unsupported message:

- `config show`, `config set`, `config path`, `config reset`
- `profile list`, `profile add`, `profile remove`, `profile use`
- `auth logout`, `auth status`, `auth revoke`

## Supported LLM providers

`tnav connect` currently supports named provider instances for:

- `ollama` (default base URL: `http://localhost:11434`)
- `openai` (default base URL: `https://api.openai.com/v1`)
- `openai-compatible` (default base URL: `http://localhost:1234`)

The OpenAI-compatible path matches local servers such as LM Studio that expose the
OpenAI-style `/v1` API.

## Supported release targets

Release packaging is configured for:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`

These are the Linux and macOS targets configured for `cargo-dist` release builds.

## Quick start

### Install

The shell installer URL only works when the published GitHub Release includes an uploaded
`tnav-installer.sh` asset. A manually created release or a release that only contains GitHub's
auto-generated source archives will still make `releases/latest/download/tnav-installer.sh`
return `404`.

The current release pipeline generates that installer with `cargo-dist` `0.31.0`.

For now, build from source:

```bash
cargo build
```

After the next successful GitHub Release is published, the shell installer will be available at:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/noizbuster/tnav/releases/latest/download/tnav-installer.sh | sh
```

### LLM command flow

```bash
tnav connect
tnav model
tnav show current directory
tnav
```

- `tnav connect` adds or manages an LLM provider instance.
- `tnav model` selects a model for the active provider. If you pass a value, it saves that model directly; if you omit it, `tnav` queries the provider and prompts you to choose.
- `tnav show current directory` sends a natural-language request to the configured LLM.
- Plain `tnav` opens an interactive prompt for the request text.

In interactive mode, plain `tnav` can guide you into `connect` and `model` setup automatically if nothing is configured yet.

### Auth/profile flow

```bash
tnav init
tnav auth login
tnav doctor
```

Typical usage:

- run `tnav init` to create a saved profile
- if the profile uses OAuth, finish sign-in with `tnav auth login`
- if the profile uses API keys, `init` can capture one immediately and `tnav auth api-key` can replace it later
- run `tnav doctor` to confirm config, keyring access, browser capability, localhost callback binding, and current auth state

## Command guide

### `tnav init`

`init` is an interactive wizard that writes profile config to `config.toml` and can store a secret in the system keyring.

The wizard currently supports these setup paths:

- API key only
- OAuth only
- both OAuth and API key

Provider choices exposed by the wizard today:

- API key profiles: OpenAI, Anthropic, or a custom provider name/base URL
- OAuth profiles: GitHub or a fully custom OAuth provider

For custom OAuth providers, the wizard prompts for:

- provider name
- optional base URL
- OAuth client ID
- authorization URL
- token URL
- optional revocation URL
- default scopes
- redirect host and redirect path
- whether the browser should open automatically during login

### `tnav auth api-key`

Stores or replaces an API key for the selected profile in secure storage. If a key already exists, `tnav` asks before overwriting unless `--yes` is provided.

### `tnav auth login`

Runs a PKCE-based OAuth authorization-code flow for OAuth profiles:

1. load the selected profile
2. validate provider configuration
3. start a localhost callback server on a loopback address and ephemeral port
4. build the authorization URL with PKCE and CSRF state
5. open the browser automatically, or print the URL when browser launch is disabled or fails
6. wait for the callback
7. exchange the authorization code for tokens
8. store token data and metadata in the system keyring

OAuth redirect hosts are validated as loopback-only values such as `127.0.0.1`, `localhost`, or `::1`.

### `tnav doctor`

`doctor` checks the current environment and selected profile state. The implemented checks include:

- whether profile config exists and loads
- whether the system keyring is reachable
- whether a browser opener appears to be available
- whether `tnav` can bind a localhost callback socket
- whether the active or requested profile resolves correctly
- whether the expected API key or OAuth token is present
- whether stored OAuth metadata indicates an expired token

### `tnav connect`

`connect` manages `llm.toml` through an interactive menu. The current flow can:

- add a new provider instance
- connect an existing provider instance
- edit a provider instance name or base URL
- replace the stored OpenAI API key for an OpenAI instance
- delete a provider instance

Multiple instances of the same provider type are supported through unique saved names.

### `tnav model [MODEL]`

Sets the model for the active LLM provider.

- with an argument: saves that model name directly
- without an argument: calls the active provider's model-list endpoint and prompts you to choose

### Prompt flow: `tnav <request>` or plain `tnav`

The prompt flow currently does all of the following:

- builds an LLM request that includes shell, OS, OS version, and architecture context
- asks the configured provider for shell code
- streams the response in interactive mode when the provider supports streaming
- shows a command preview
- lets you execute, edit, or cancel
- executes approved commands via `sh -c`

If no LLM provider or model is configured, interactive prompt mode can bootstrap you into `tnav connect` and `tnav model` first.

## Configuration and secret storage

`tnav` intentionally separates non-secret config from secrets.

### `config.toml`

Profile/auth configuration is stored in `config.toml`, resolved through the `directories` crate under the `com/noizbuster/tnav` app identity.

Current contents include:

- active profile name
- per-profile provider name
- auth method (`api_key` or `oauth`)
- optional base URL
- default scopes
- OAuth endpoints and client ID
- redirect host and redirect path
- UI preference for automatic browser opening

Commands that operate on this file honor `--config`.

### `llm.toml`

LLM connection state is stored separately in `llm.toml` in the standard config directory.

Current contents include:

- active LLM provider instance name
- list of configured provider instances
- provider kind
- saved model name
- optional custom base URL
- request timeout value

The current implementation loads and saves `llm.toml` from the standard config location; there is no separate CLI flag to override it.

### Secure secret storage

Secrets are stored via the system keyring using the `tnav` service name.

Stored secret types include:

- profile API keys
- OAuth access tokens
- OAuth refresh tokens
- OAuth token metadata used for expiry checks
- OpenAI provider API keys for `tnav connect`

On Unix, config directories are created with `0700` permissions and config files with `0600` permissions where supported.

## Current interactive/non-interactive behavior

The CLI has partial non-interactive support in the current implementation.

- `tnav init` requires interactive answers
- `tnav auth api-key` requires an interactive key prompt
- `tnav connect` is currently interactive
- `tnav model some-model` can be used directly if a provider is already configured
- `tnav <request>` can run non-interactively when provider and model are already configured
- plain `tnav` requires interactive prompt entry

## Troubleshooting

### Browser did not open during OAuth login

- rerun with `--no-browser` to print the authorization URL instead
- install a browser opener utility on Linux or set `BROWSER`
- complete the browser step on the same machine because the callback listener is local

### Keyring errors

- rerun `tnav doctor`
- verify your desktop/session keyring is available
- API key storage, OAuth login, and OpenAI provider auth depend on working keyring access

### `run 'tnav connect' first` or `run 'tnav model' first`

- run `tnav connect` to create an LLM provider instance
- run `tnav model` to save a model for the active provider
- retry the prompt request after both steps succeed

### OAuth callback timeout or bind failure

- confirm loopback networking is allowed on the current machine
- retry in an environment that can bind localhost sockets
- verify the configured redirect host is still a loopback address

## Developer guide

### Prerequisites

- Rust stable toolchain
- a working C compiler (`cc`) for native dependencies

Install Rust with `rustup` if needed:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable
rustup update
```

### Local verification commands

These match the current CI workflow:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo check --all-features
```

Useful local commands while developing:

```bash
cargo run -- --help
cargo run -- doctor
cargo run -- connect
cargo run -- model
```

### CI and releases

- CI runs format, clippy, tests, and `cargo check`
- tagged release builds use `cargo-dist` `0.31.0` via the `dist` CLI, and the `Release` workflow can also be dispatched manually for an existing tag to upload missing assets
- a successful release publish run will attach per-target archives, checksums, and a shell installer

Useful local release-tooling checks:

```bash
dist manifest --tag v0.1.0 --artifacts=all --no-local-paths --output-format=json --allow-dirty
dist build --tag v0.1.0 --target x86_64-unknown-linux-gnu --artifacts=all --allow-dirty
```

---

<p align="center">
  <strong>Supervised by NoizBuster, Written by OpenCode</strong>
</p>

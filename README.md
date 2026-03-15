# tnav

`tnav` is a Rust CLI scaffold for interactive local authentication setup and checks.

Current behavior is intentionally narrow. The implemented flows cover project init,
API key storage, browser-based OAuth login, and environment diagnostics.

## What `tnav` supports today

- `init`
- `auth api-key`
- `auth login`
- `doctor`
- `version`

The remaining command groups exist in the parser but return a clear unsupported
error message in this phase:

- `config`
- `profile`
- `auth logout`, `auth status`, `auth revoke`

## Supported platforms

Release packaging targets are configured for these architectures:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`

These map to Linux and macOS binaries and a release shell installer.

## Install

Install from the latest GitHub release with:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/noizbuster/tnav/releases/latest/download/tnav-installer.sh | sh
```

The workflow builds per target artifacts, publishes checksums, and includes the
`tnav-installer.sh` shell installer asset through `cargo-dist`.

## Local build and verification commands

Use these from the repository root:

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo run -- --help
cargo run -- init
```

If you run all of them in your environment, they should match the development
intended behavior. In some environments, external toolchain prerequisites can still
block `cargo test` and `cargo check`.

## Developer Guide

This section covers the setup required for contributing to `tnav` development.

### Prerequisites

#### Rust Toolchain

Install Rust using `rustup`:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

After installation, ensure you have the stable toolchain:

```bash
rustup default stable
rustup update
```

#### C Compiler (`cc`)

A C compiler is required for building some native dependencies (e.g., `ring` for TLS).

**Linux (Debian/Ubuntu):**

```bash
sudo apt update
sudo apt install build-essential
```

**Linux (Fedora/RHEL/CentOS):**

```bash
sudo dnf install gcc
# or on older systems:
sudo yum install gcc
```

**Linux (Arch Linux):**

```bash
sudo pacman -S gcc
```

**macOS:**

On macOS, `cc` is provided by Xcode Command Line Tools:

```bash
xcode-select --install
```

This will install `clang` which is symlinked as `cc`.

Verify your C compiler installation:

```bash
cc --version
```

#### rust-analyzer (IDE Support)

`rust-analyzer` provides IDE features like autocompletion, go to definition, and inline errors.

**VS Code:**

1. Open the Extensions view (`Ctrl+Shift+X` or `Cmd+Shift+X`)
2. Search for "rust-analyzer"
3. Click Install

Alternatively, install via command line:

```bash
code --install-extension rust-lang.rust-analyzer
```

**Other Editors:**

- **Neovim/Vim**: Use `nvim-lspconfig` or `coc.nvim` with rust-analyzer
- **Emacs**: Use `lsp-mode` or `eglot` with rust-analyzer
- **Helix**: Built-in LSP support, ensure `rust-analyzer` is in PATH

Install `rust-analyzer` binary manually (if needed):

```bash
rustup component add rust-analyzer
```

Or download the latest binary from the [rust-analyzer releases page](https://github.com/rust-lang/rust-analyzer/releases).

### Development Workflow

1. Clone the repository:

```bash
git clone https://github.com/noizbuster/tnav.git
cd tnav
```

2. Build the project:

```bash
cargo build
```

3. Run tests:

```bash
cargo test
```

4. Run linting:

```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

5. Run the CLI locally:

```bash
cargo run -- --help
```

## Current command usage

```bash
tnav --help
tnav init
tnav auth api-key
tnav auth login
tnav doctor
tnav version
```

`init` writes non-secret config and can save an API key into the secret store.
`auth login` starts the OAuth flow for OAuth profiles and saves token data.
`doctor` runs quick health checks for config, browser capability, local callback
ability, keyring access, and stored auth state.

## Config storage

`tnav` writes non-secret configuration to a `config.toml` file resolved with the
`directories` crate under the `com/noizbuster/tnav` project scope.

The config model contains:

- active profile name
- per-profile settings such as provider, auth method, OAuth endpoints, scopes,
  and redirect host/path
- no API keys or tokens

The configuration is serialized as TOML and saved with restrictive permissions
where supported (`0600` for files on Unix).

## Secret storage

Secrets are intentionally separated from config and stored via `keyring`:

- API key secrets
- OAuth access tokens
- OAuth refresh tokens
- OAuth metadata used for expiry checks

The keyring service name is `tnav`, with account labels based on profile and
secret kind. If keyring is unavailable, related commands return a clear error and
`doctor` marks this as an issue.

## OAuth login flow at a high level

For OAuth profiles, `tnav auth login` follows this sequence:

1. Load profile from config
2. Validate provider config
3. Start a localhost callback server on loopback host and a random port
4. Build authorization URL with PKCE verifier/challenge and CSRF state
5. Open the authorization URL in the browser
6. Receive the callback and validate CSRF state
7. Exchange the authorization code for tokens
8. Persist token set metadata in secure storage

The redirect URI uses the configured loopback host and path plus the selected
ephemeral port.

## Troubleshooting

For command-line install and auth issues, start with `tnav doctor` and then apply
the specific fixes below.

### Browser did not open

- If CLI output shows manual URL instructions, copy and open the URL in a browser.
- Install a browser opener utility on Linux (`xdg-open`, `gio`, `sensible-browser`,
  etc.), or set `BROWSER` in the environment so `tnav` can attempt a launch.
- You must complete the browser step on the same machine where `tnav` is running, because the callback listener is local (`127.0.0.1`).

### Keyring unavailable (Linux session)

- Re-run `tnav doctor` to confirm keyring availability.
- Re-run after fixing keyring service/tooling permissions.
- `auth api-key` and `auth login` require working secret storage for success.

### Localhost callback blocked or port bind failure

- Confirm no local firewall or security policy blocks `127.0.0.1` loopback bind.
- Retry with a different session that can open a localhost listener.
- If this keeps failing, inspect local environment restrictions on ephemeral
  loopback ports and temporary socket binds.

### Token expired or revoked

- `doctor` reports token state as `expired` or `missing` when metadata shows stale
  state.
- Re-run `tnav auth login` to mint a fresh token.
- If a provider supports token revocation, rerun OAuth login to re-authorize from
  scratch.

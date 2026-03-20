#!/usr/bin/env bash
# Run all CI checks locally
# Usage: ./scripts/ci.sh

set -euo pipefail

echo "🔍 Running CI checks..."
echo

echo "📦 1/4 cargo fmt --check"
cargo fmt --all -- --check
echo "✅ fmt passed"
echo

echo "📦 2/4 cargo clippy"
cargo clippy --all-targets --all-features -- -D warnings
echo "✅ clippy passed"
echo

echo "📦 3/4 cargo test"
cargo test --all-targets --all-features
echo "✅ tests passed"
echo

echo "📦 4/4 cargo check"
cargo check --all-features
echo "✅ check passed"
echo

echo "🎉 All CI checks passed!"

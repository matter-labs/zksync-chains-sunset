#!/usr/bin/env bash
set -euo pipefail

if ! command -v rustup >/dev/null 2>&1; then
  echo "error: rustup is required but not installed" >&2
  exit 1
fi
if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo is required but not installed" >&2
  exit 1
fi
if ! command -v rustc >/dev/null 2>&1; then
  echo "error: rustc is required but not installed" >&2
  exit 1
fi

# RUST_TOOLCHAIN="$(awk -F'"' '/^channel =/ { print $2 }' rust-toolchain.toml)"
# if ! rustup run "$RUST_TOOLCHAIN" rustc --version >/dev/null 2>&1; then
#   echo "error: rust toolchain '$RUST_TOOLCHAIN' is not installed via rustup" >&2
#   exit 1
# fi
# if ! rustup run "$RUST_TOOLCHAIN" cargo fmt --version >/dev/null 2>&1; then
#   echo "error: rustfmt component is missing for toolchain '$RUST_TOOLCHAIN'" >&2
#   exit 1
# fi
# if ! rustup run "$RUST_TOOLCHAIN" cargo clippy --version >/dev/null 2>&1; then
#   echo "error: clippy component is missing for toolchain '$RUST_TOOLCHAIN'" >&2
#   exit 1
# fi

if ! command -v forge >/dev/null 2>&1; then
  echo "error: foundry (forge) is required but not installed" >&2
  exit 1
fi
if ! command -v cast >/dev/null 2>&1; then
  echo "error: foundry (cast) is required but not installed" >&2
  exit 1
fi
if ! command -v bun >/dev/null 2>&1; then
  echo "error: bun is required but not installed" >&2
  exit 1
fi

echo "Installing frontend dependencies..."
(cd apps/frontend && bun install)

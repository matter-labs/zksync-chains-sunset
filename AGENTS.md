# AGENTS

## Rust
- Use Rust 2024 edition for all crates.
- Do not use `mod.rs`; use `module_name.rs` instead.
- Prefer explicit module trees with `pub mod module_name;` in parent modules.
- When a crate has both a library and a binary, keep logic in `src/lib.rs` and keep `src/main.rs` as a thin entrypoint.
- Keep crate boundaries clear (no cross-crate `path` dependencies unless necessary).
- Use `just fix`, `just check`, and `just test` as the canonical Rust maintenance commands in this repo instead of invoking `cargo fmt`, `cargo clippy`, or `cargo test` directly.
- `just test` uses `cargo nextest run` for Rust tests in this repo instead of `cargo test`.

## Justfiles
- Use `justfile` as the canonical entrypoint for common commands.
- Ensure frequently used commands are available: build, test, lint/fmt, run/dev.
- As new commands become common (including by agents), add them to the relevant `justfile`.

## Execution
- Prefer running commands in this workspace to validate changes before telling the user to run them.
- If a command fails due to sandboxing, permissions, network, or DNS issues, immediately request a rerun with escalated permissions (instead of asking the user to do it manually).

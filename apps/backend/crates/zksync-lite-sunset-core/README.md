# zksync-lite-sunset-core

Core library shared by the CLI and server.

## Responsibilities

This crate owns the backend business logic for:

- snapshot CSV import
- claim entry and proof materialization
- dev fixture CSV generation for seeded-address flows
- runtime state seeding used by the seeded dev stack

For the seeded dev path, shell scripts should invoke this logic through the CLI rather than linking to `core` directly.

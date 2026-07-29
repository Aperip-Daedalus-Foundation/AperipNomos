# AperipNomos Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a pure-Rust, RNMDB-backed dual-port backend for uploading and publishing open-source license texts.

**Architecture:** Two Axum routers share a bounded handle to one database actor. The actor owns the only durable RNMDB session, validates persisted rows at startup, and commits each mutation transactionally.

**Tech Stack:** Rust 1.95, Axum 0.8, Tokio 1, RNMDB git revision `013ec2f`, Docker/Compose.

---

### Task 1: Upload Domain

**Files:**
- Create: `tests/domain_contract.rs`
- Create: `src/domain.rs`
- Create: `src/lib.rs`

- [ ] Write tests that call `LicenseDraft::from_upload` with valid, empty,
  oversized, non-UTF-8, explicit-slug, and non-ASCII filename inputs.
- [ ] Run `cargo test --test domain_contract` and confirm compilation fails
  because the domain module does not exist.
- [ ] Implement `LicenseDraft`, `LicenseValidationError`, slug normalization,
  title derivation, SHA-256 hashing, and the 1 MiB boundary.
- [ ] Run `cargo test --test domain_contract` and confirm every domain contract
  passes.
- [ ] Commit with `feat(domain): validate uploaded licenses`.

### Task 2: Configuration And Secrets

**Files:**
- Create: `tests/config_contract.rs`
- Create: `src/config.rs`

- [ ] Write tests for separate public/admin sockets, 64-hex-character RNMDB
  keys, minimum 32-byte administrator tokens, and secret-file trimming.
- [ ] Run `cargo test --test config_contract` and confirm missing configuration
  types cause the expected failure.
- [ ] Implement environment loading for `PUBLIC_BIND_ADDR`,
  `ADMIN_BIND_ADDR`, `RNMDB_PATH`, `RNMDB_PAGE_KEY_FILE`, and
  `ADMIN_TOKEN_FILE` without logging secret values.
- [ ] Run `cargo test --test config_contract` and confirm all cases pass.
- [ ] Commit with `feat(config): load dual-port service secrets`.

### Task 3: RNMDB Actor

**Files:**
- Create: `tests/store_contract.rs`
- Create: `src/storage.rs`
- Create: `src/store.rs`

- [ ] Write a real-file test that starts the actor, creates two licenses,
  lists them in title order, fetches one, rejects a duplicate, deletes one,
  shuts down, restarts, and verifies persisted state.
- [ ] Run `cargo test --test store_contract` and confirm actor symbols are
  missing.
- [ ] Implement the RNMDB owner lock, schema, row decoding, bounded commands,
  transactional create/delete, recovery, and orderly shutdown.
- [ ] Run `cargo test --test store_contract` and confirm the persistence
  contract passes.
- [ ] Commit with `feat(storage): persist licenses with rnmdb`.

### Task 4: Public And Administrator Routers

**Files:**
- Create: `tests/http_contract.rs`
- Create: `src/http/mod.rs`
- Create: `src/http/admin.rs`
- Create: `src/http/public.rs`
- Create: `src/http/error.rs`

- [ ] Write router tests for public list/detail/not-found, absent public
  mutation routes, rejected missing/invalid bearer tokens, multipart upload,
  duplicate conflict, and authenticated deletion.
- [ ] Run `cargo test --test http_contract` and confirm router constructors are
  missing.
- [ ] Implement JSON response types, constant-time bearer authentication,
  multipart parsing with a 1 MiB body limit, stable error envelopes, request
  tracing, and separate routers.
- [ ] Run `cargo test --test http_contract` and confirm the HTTP contract
  passes.
- [ ] Commit with `feat(api): expose isolated public and admin routes`.

### Task 5: Dual Listener Runtime

**Files:**
- Create: `src/main.rs`
- Create: `tests/runtime_contract.rs`

- [ ] Write tests for binding two distinct ephemeral sockets and cleanly
  stopping both listeners through one shutdown signal.
- [ ] Run `cargo test --test runtime_contract` and confirm runtime helpers are
  missing.
- [ ] Implement startup, tracing, two listener tasks, readiness, coordinated
  shutdown, and actor termination.
- [ ] Run `cargo test --test runtime_contract` and confirm shutdown completes.
- [ ] Commit with `feat(server): run isolated dual-port listeners`.

### Task 6: Container Delivery And Final Gates

**Files:**
- Create: `Dockerfile`
- Create: `docker-compose.yml`
- Create: `.dockerignore`
- Create: `.env.example`
- Modify: `README.md`

- [ ] Add a cached Rust builder and a non-root Debian runtime exposing ports
  28740 and 28741.
- [ ] Add Compose secrets, persistent RNMDB volume, read-only root filesystem,
  health check, and explicit dual host-port mappings.
- [ ] Document secret creation, local Cargo run, Compose run, and curl examples
  for upload/list/detail/delete.
- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run `cargo check --workspace --all-targets --locked`.
- [ ] Run `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- [ ] Run `cargo test --workspace --locked`.
- [ ] Run `cargo build --release --locked`.
- [ ] Run `docker compose config --quiet` and `docker build -t aperip-nomos:local .`.
- [ ] Commit with `chore(container): add hardened dual-port delivery`.

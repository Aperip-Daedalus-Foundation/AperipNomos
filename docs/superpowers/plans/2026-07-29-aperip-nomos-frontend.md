# AperipNomos Frontend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a responsive public license archive and memory-only-token administrator panel from the existing Rust binary, then deploy the locally built image without replacing RNMDB data or secrets.

**Architecture:** Axum serves embedded HTML, CSS, JavaScript, and SVG assets from listener-specific routes. Dependency-free browser modules call the existing same-origin JSON APIs and render untrusted data only through DOM text properties. Public and administrator listener isolation, API contracts, and Docker persistence remain unchanged.

**Tech Stack:** Rust 2024, Axum 0.8, embedded static assets via `include_str!`, semantic HTML, plain CSS, dependency-free ES2022 JavaScript, RNMDB, Docker Compose.

---

## File Map

- `src/http/assets.rs`: embedded asset response type, cache and security headers, listener-specific asset handlers.
- `src/http/assets/public.html`: public catalog document shell.
- `src/http/assets/detail.html`: public license detail document shell.
- `src/http/assets/admin.html`: administrator document shell and accessible confirmation dialog.
- `src/http/assets/app.css`: shared responsive editorial visual system.
- `src/http/assets/public.js`: catalog loading, filtering, counting, and row rendering.
- `src/http/assets/detail.js`: slug lookup, metadata/body rendering, and copy feedback.
- `src/http/assets/admin.js`: memory-only token gate, uploads, list refresh, lock, and confirmed deletion.
- `src/http/assets/favicon.svg`: self-contained registry mark.
- `src/http/public.rs`: public page and public asset routes.
- `src/http/admin.rs`: administrator page and administrator asset routes.
- `src/http/mod.rs`: register the asset module.
- `tests/frontend_contract.rs`: response, isolation, header, and safe-source integration contracts.
- `README.md`: replace the backend-phase notice with deployed UI entry points and SSH-tunnel guidance.

### Task 1: Embedded Asset and Route Contracts

**Files:**
- Create: `tests/frontend_contract.rs`
- Create: `src/http/assets.rs`
- Modify: `src/http/mod.rs`
- Modify: `src/http/public.rs`
- Modify: `src/http/admin.rs`

- [ ] **Step 1: Write failing route tests**

Create a temporary store using the existing `spawn_store` helper and assert:

```rust
let public_home = request(&public, "/").await;
assert_eq!(public_home.status(), StatusCode::OK);
assert_content_type(&public_home, "text/html; charset=utf-8");
assert_security_headers(&public_home);

let public_detail = request(&public, "/licenses/mit").await;
assert_eq!(public_detail.status(), StatusCode::OK);

let admin_home = request(&admin, "/").await;
assert_eq!(admin_home.status(), StatusCode::OK);
assert!(body_text(admin_home).await.contains("id=\"token-form\""));

assert_eq!(request(&public, "/assets/admin.js").await.status(), StatusCode::NOT_FOUND);
assert_eq!(request(&admin, "/assets/public.js").await.status(), StatusCode::NOT_FOUND);
```

The security helper must require the exact self-only Content Security Policy,
`nosniff`, `no-referrer`, and `DENY` frame headers.

- [ ] **Step 2: Run the focused test and confirm RED**

Run: `cargo test --test frontend_contract --locked --offline`

Expected: compilation or route assertions fail because the asset module and page routes do not exist.

- [ ] **Step 3: Add the embedded response boundary**

Implement a small `EmbeddedAsset` response in `src/http/assets.rs` with these constants and methods:

```rust
const CSP: &str = "default-src 'self'; base-uri 'none'; connect-src 'self'; font-src 'self'; form-action 'self'; frame-ancestors 'none'; img-src 'self'; object-src 'none'; script-src 'self'; style-src 'self'";

pub(super) async fn public_home() -> EmbeddedAsset;
pub(super) async fn public_detail() -> EmbeddedAsset;
pub(super) async fn admin_home() -> EmbeddedAsset;
pub(super) async fn stylesheet() -> EmbeddedAsset;
pub(super) async fn public_script() -> EmbeddedAsset;
pub(super) async fn detail_script() -> EmbeddedAsset;
pub(super) async fn admin_script() -> EmbeddedAsset;
pub(super) async fn favicon() -> EmbeddedAsset;
```

HTML responses use `Cache-Control: no-store`; static assets use `Cache-Control: public, max-age=3600`. Every response receives the browser security headers.

- [ ] **Step 4: Register isolated listener routes**

Public router routes:

```text
GET /
GET /licenses/{slug}
GET /assets/app.css
GET /assets/public.js
GET /assets/detail.js
GET /favicon.svg
```

Administrator router routes:

```text
GET /
GET /assets/app.css
GET /assets/admin.js
GET /favicon.svg
```

Keep the existing API routes, auth middleware, fallbacks, body limit, and trace layer unchanged.

- [ ] **Step 5: Run focused tests and commit**

Run: `cargo test --test frontend_contract --locked --offline`

Expected: route, isolation, content type, and security header assertions pass once the asset files from Tasks 2 and 3 exist.

Commit: `feat(frontend): serve isolated embedded interfaces`

### Task 2: Public Catalog and License Detail

**Files:**
- Create: `src/http/assets/public.html`
- Create: `src/http/assets/detail.html`
- Create: `src/http/assets/app.css`
- Create: `src/http/assets/public.js`
- Create: `src/http/assets/detail.js`
- Create: `src/http/assets/favicon.svg`
- Test: `tests/frontend_contract.rs`

- [ ] **Step 1: Extend the failing source contract**

Request the public JavaScript assets and assert that they contain the real API paths, `textContent`, loading/error handling, and no `innerHTML`, `localStorage`, or `sessionStorage`. Assert that the HTML documents contain labeled controls, `<noscript>`, live regions, and these stable IDs:

```text
license-filter, license-count, license-list, catalog-status
license-title, license-slug, license-source, license-digest, license-uploaded
license-body, copy-license, copy-status, detail-status, detail-content
```

- [ ] **Step 2: Run the focused test and confirm RED**

Run: `cargo test --test frontend_contract public_assets --locked --offline`

Expected: failure because the public documents and modules are absent or incomplete.

- [ ] **Step 3: Implement the public documents**

Use semantic headers, main content, real empty/loading text, and rule-separated rows. `public.html` loads `/assets/public.js` with `type="module"`; `detail.html` loads `/assets/detail.js`. Both link `/assets/app.css` and `/favicon.svg`. Do not add promotional sections or example licenses.

- [ ] **Step 4: Implement safe public behavior**

`public.js` fetches `/api/licenses`, renders links as `/licenses/${encodeURIComponent(slug)}`, filters by title/slug/source filename, updates a live count, and exposes loading, empty, and failure states. `detail.js` extracts the final decoded path segment, fetches `/api/licenses/{slug}`, fills metadata with `textContent`, formats the upload date with `Intl.DateTimeFormat`, and copies the exact body through `navigator.clipboard.writeText`.

All API errors resolve to short user-facing states; neither module inserts fetched content as HTML.

- [ ] **Step 5: Implement the shared editorial stylesheet**

Define semantic custom properties for canvas, ink, muted ink, rule, accent, danger, focus, spacing, and type families. Cover 320/768/1024/1440 widths, wrapping `<pre>`, visible `:focus-visible`, stable hover states, dialog styling, form errors, disabled controls, and `prefers-reduced-motion: reduce`. Use no remote font or image URL.

- [ ] **Step 6: Run focused tests and commit**

Run: `cargo test --test frontend_contract public_assets --locked --offline`

Expected: public asset contracts pass.

Commit: `feat(frontend): add public license registry`

### Task 3: Administrator Token Gate and Manager

**Files:**
- Create: `src/http/assets/admin.html`
- Create: `src/http/assets/admin.js`
- Test: `tests/frontend_contract.rs`

- [ ] **Step 1: Extend the failing administrator source contract**

Assert that the administrator document contains labeled token, file, title, and slug inputs; live status nodes; list/count nodes; lock control; and a native confirmation dialog. Assert the script contains the three existing administrator API operations and does not contain `localStorage`, `sessionStorage`, `document.cookie`, URL token parameters, or `innerHTML`.

Required IDs:

```text
token-gate, token-form, admin-token, token-error, manager, lock-admin
upload-form, license-file, upload-title, upload-slug, upload-status
admin-list, admin-count, admin-status, delete-dialog, delete-license-name
confirm-delete, cancel-delete
```

- [ ] **Step 2: Run the focused test and confirm RED**

Run: `cargo test --test frontend_contract admin_assets --locked --offline`

Expected: failure because the administrator assets are absent or incomplete.

- [ ] **Step 3: Implement token and list behavior**

Keep `let adminToken = ""` only in module scope. On token form submission, call authenticated `GET /api/admin/licenses`; reveal the manager only on `200`, otherwise clear both the variable and password input. `lock-admin` clears the same state and returns focus to the token input.

- [ ] **Step 4: Implement upload and confirmed deletion**

Build `FormData` from the required file and non-empty optional fields, then call authenticated `POST /api/admin/licenses`. Render server error messages without raw bodies. For deletion, store only the pending slug in module memory, name the record in `<dialog>`, and call authenticated `DELETE /api/admin/licenses/{slug}` only after explicit confirmation. Refresh the list after successful mutations.

- [ ] **Step 5: Run focused tests and commit**

Run: `cargo test --test frontend_contract admin_assets --locked --offline`

Expected: administrator source and isolation contracts pass.

Commit: `feat(frontend): add memory-only admin console`

### Task 4: Documentation, Browser QA, and Deployment

**Files:**
- Modify: `README.md`
- Verify: all frontend and backend files

- [ ] **Step 1: Update operational documentation**

Document the public root URL, public detail route, administrator root URL, memory-only token behavior, and SSH tunnel requirement. Remove the statement that visual surfaces are a future phase. Keep all existing secret, RNMDB, and Docker warnings.

- [ ] **Step 2: Run local quality gates**

Run in order:

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --all-targets --locked --offline
cargo build --release --locked --offline
docker compose --env-file .env.example config --quiet
```

Expected: every command exits zero and all tests pass.

- [ ] **Step 3: Run a real local container and browser review**

Build a `linux/amd64` immutable image, start it with temporary secrets and a fresh volume, upload two real small license fixtures through the administrator UI, then verify at 320, 768, 1024, and 1440 CSS pixels:

```text
catalog load and filtering
detail navigation and exact body copy
token rejection and unlock
upload success
confirmed deletion and cancellation
visible focus and no horizontal overflow
no browser console errors
```

Delete the temporary container, volume, fixtures, and secrets after the review.

- [ ] **Step 4: Independent review and final commit**

Review the full diff for listener isolation, token persistence, XSS sinks, CSP compatibility, responsiveness, API compatibility, and repository hygiene. Resolve every blocking issue and commit documentation or review fixes with a Conventional Commit subject.

- [ ] **Step 5: Deploy without rebuilding remotely**

Export the verified immutable image and its SHA-256 checksum. Upload only the image archive, checksum, release metadata, and remote Compose file. On `43.165.184.253`, preserve `/opt/aperip-nomos/shared/secrets` and `aperip-nomos_rnmdb-data`, load the image, update the Compose image tag, and use `up --no-build --pull never -d --wait`.

- [ ] **Step 6: Verify production**

Require public `/`, `/api/licenses`, and `/health/ready` to return `200` externally. Require administrator `/`, authenticated list, upload, and delete to work over `127.0.0.1:54872`; the same port must remain unreachable from outside. Restart the container and confirm RNMDB records persist, the image ID matches the local build, and the container remains non-root, read-only, capability-free, and healthy.

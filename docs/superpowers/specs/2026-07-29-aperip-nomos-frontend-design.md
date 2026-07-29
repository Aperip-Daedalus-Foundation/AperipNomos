# AperipNomos Frontend Design

## Purpose

AperipNomos needs a small, production-ready interface for browsing an
organization's published open-source licenses and administering the archive.
The interface must use the real RNMDB-backed API, ship inside the existing Rust
binary, and add no Node.js build or runtime dependency.

## Architecture

HTML, CSS, JavaScript, and the SVG favicon are source assets embedded into the
Rust binary with `include_str!`. Axum serves those immutable assets from both
listeners where required. Public pages fetch the existing public JSON API from
the public origin. The administrator page fetches the existing authenticated
API from the administrator origin.

The browser code is dependency-free ES2022 JavaScript. It creates user-derived
content with DOM properties such as `textContent`; it does not interpolate
license data into HTML. The administrator bearer token is held only in a module
variable and is never written to local storage, session storage, cookies, URLs,
or rendered page content.

## Visual System

The product uses an editorial registry style informed by Swiss modernism:

- canvas `#F5F7F5`, ink `#151A17`, muted ink `#59645E`;
- rules `#D8DED9`, accent `#0B6B57`, danger `#B42318`;
- system serif headings, system sans-serif controls, and system monospace
  license text so the deployment requires no third-party font requests;
- one content width, thin rules, restrained spacing, and no decorative cards,
  gradients, stock imagery, marketing hero, or invented statistics;
- visible keyboard focus, stable hover states, and motion disabled when
  `prefers-reduced-motion` is active.

The layout must remain usable without horizontal scrolling at 320, 768, 1024,
and 1440 CSS pixels.

## Public Catalog

`GET /` serves the archive index. It contains the AperipNomos identity, one
plain description, a labeled client-side filter, a live result count, and a
rule-separated list populated from `GET /api/licenses`. Each row links to
`/licenses/{slug}` and shows only real title, slug, source filename, and digest
metadata. The empty, loading, and service-error states are explicit.

`GET /licenses/{slug}` serves the detail shell. It fetches
`GET /api/licenses/{slug}`, shows real metadata, and renders the license body in
a wrapping `<pre>` using `textContent`. A copy button writes the exact body to
the clipboard and exposes a short accessible status. Missing records have a
specific not-found presentation and a link back to the archive.

## Administrator Panel

`GET /` on the administrator listener serves a token gate. Submitting a token
validates it with `GET /api/admin/licenses`; success reveals the manager and
failure clears the in-memory token.

The manager provides:

- a compact list of real licenses and upload timestamps;
- an upload form with required text file and optional title and slug;
- progress and API error feedback without exposing response internals;
- a confirmation dialog that names the license before deletion;
- a lock action that immediately clears the token and protected UI state.

Uploads use the existing multipart contract and are limited by the existing
backend. Deletion uses the existing authenticated `DELETE` endpoint. The list
refreshes only after a successful mutation.

## HTTP and Security

The JSON API contract is unchanged. Page and asset responses set correct
content types and browser security headers, including a self-only Content
Security Policy, `X-Content-Type-Options: nosniff`, `Referrer-Policy:
no-referrer`, and frame denial. HTML is not cached; static assets may use a
short public cache. No external script, stylesheet, image, or font origin is
allowed.

Only the public listener exposes public pages and public JavaScript. Only the
administrator listener exposes the administrator page and administrator
JavaScript. Shared CSS and favicon assets are safe on both listeners. API
authentication and listener binding remain unchanged.

## Error Handling and Accessibility

Every asynchronous action has loading, success, empty, and failure behavior.
Controls use semantic labels, form errors are announced through `aria-live`,
dialogs restore focus, and all functions are usable with a keyboard. Color is
not the sole status indicator. The page remains understandable if JavaScript
fails by showing a short `<noscript>` explanation.

## Verification

Rust integration tests prove route isolation, content types, security headers,
HTML shells, and unchanged JSON behavior. Static-source contract tests guard
the memory-only token policy and safe DOM rendering. The complete Rust format,
Clippy, test, release-build, Docker Compose, and image gates run before
deployment.

A real browser verifies desktop and mobile layouts, filtering, detail loading,
copy feedback, token unlock, upload, confirmed deletion, focus visibility, and
the absence of console errors. Deployment reuses the existing RNMDB secrets and
volume, loads a locally built immutable image, and validates both local and
external listener boundaries after restart.

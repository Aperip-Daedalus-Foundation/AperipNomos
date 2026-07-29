# AperipNomos Backend Design

## Scope

This phase builds the complete persistence and HTTP backend for an open-source
license archive. It deliberately leaves visual styling and browser-side
interaction for the next phase. No Node.js or JavaScript build chain is used.

## Architecture

One Tokio process starts two Axum listeners. The public listener exposes
read-only catalog APIs on port 28740. The administrator listener exposes
authenticated mutation APIs on port 28741. Both listeners share a bounded
actor handle; a dedicated thread exclusively owns one encrypted RNMDB
`LocalSession`, serializing every database operation and preventing unsafe
multi-session writes.

The database dependency is pinned to RNMDB revision
`013ec2f48a1dab89997430d72c2b176be2c29d47` from
`https://github.com/czxieddan/RNovModularDB.git`.

## Data Model

The `licenses` table stores an integer identifier, URL slug, display title,
base64-encoded UTF-8 body, original filename, SHA-256 digest, and upload time in
Unix milliseconds. A unique index protects slugs. Startup loads and validates
all rows before readiness is reported.

Uploaded files must be non-empty UTF-8 plain text and at most 1 MiB. Slugs are
lowercase ASCII words separated by hyphens. When no slug is supplied, the
server derives one from the filename; names with no ASCII words fall back to a
digest-based slug. Duplicate slugs return conflict and never overwrite data.

## HTTP Contract

Public listener:

- `GET /health/live`
- `GET /health/ready`
- `GET /api/licenses`
- `GET /api/licenses/{slug}`

Administrator listener:

- `GET /health/live`
- `GET /health/ready`
- `GET /api/admin/licenses`
- `POST /api/admin/licenses` as multipart fields `file`, optional `title`, and
  optional `slug`
- `DELETE /api/admin/licenses/{slug}`

Administrator API calls require `Authorization: Bearer <token>`. The token is
read from a Docker secret file and compared in constant time. The public
listener never mounts administrator routes.

## Durability And Failure Handling

Schema creation is checkpointed at startup. Mutations run in explicit RNMDB
transactions; successful `COMMIT` provides the durable checkpoint. On an
ambiguous database error, the actor closes and reopens RNMDB before accepting
further commands. Queue saturation returns `503`, validation returns `400`,
missing rows return `404`, duplicate slugs return `409`, and internal storage
details are not included in HTTP responses.

## Container Boundary

A multi-stage Dockerfile builds one Rust binary and runs it as a non-root user
with a read-only root filesystem. Compose exposes ports 28740 and 28741, mounts a
persistent database volume, and injects the RNMDB page key and administrator
token as files under `/run/secrets`.

## Verification

Domain tests cover file validation and deterministic identity derivation.
Actor tests cover create/list/get/delete and restart persistence against a real
RNMDB file. Router tests prove bearer authentication and confirm that mutation
routes are absent from the public listener. Final gates are formatting,
workspace check, warning-free Clippy, tests, release build, and Docker Compose
configuration validation.

# AperipNomos

AperipNomos is a small, self-hosted archive for publishing an organization's
open-source license texts. One Rust process owns the encrypted RNMDB database
and serves two isolated HTTP listeners:

- public catalog: `8080`
- administrator API: `8081`

The backend is currently the active implementation phase. The public and
administrator visual surfaces will be built after their layout is approved.

## Name

`Nomos` is the ancient Greek word for law, custom, and shared order. The name
connects the archive's legal subject with Aperip's role as its curator.


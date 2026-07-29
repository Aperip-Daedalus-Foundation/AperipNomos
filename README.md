# AperipNomos

AperipNomos is a small, self-hosted archive for publishing an organization's
open-source license texts. One Rust process owns the encrypted RNMDB database
and serves two isolated HTTP listeners:

- public catalog: `28740`
- administrator API: `28741`

The backend is currently the active implementation phase. The public and
administrator visual surfaces will be built after their layout is approved.

## Backend API

Public listener (`28740`):

- `GET /health/live`
- `GET /health/ready`
- `GET /api/licenses`
- `GET /api/licenses/{slug}`

Administrator listener (`28741`):

- `GET /health/live`
- `GET /health/ready`
- `GET /api/admin/licenses`
- `POST /api/admin/licenses`
- `DELETE /api/admin/licenses/{slug}`

Every administrator API request requires `Authorization: Bearer <token>`.
Uploads use `multipart/form-data` with a required `file` field and optional
`title` and `slug` fields. License files must be non-empty UTF-8 text no larger
than 1 MiB. An existing slug is never overwritten.

## Run With Docker

Create the two local secret files once. The command refuses to overwrite an
existing key:

```powershell
New-Item -ItemType Directory -Force secrets | Out-Null
$userSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
& icacls.exe secrets /inheritance:r /grant:r "*${userSid}:(OI)(CI)F" `
  "*S-1-5-18:(OI)(CI)F" "*S-1-5-32-544:(OI)(CI)F" | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Failed to restrict the secrets directory ACL.' }
if ((Test-Path secrets/rnmdb_page_key) -or (Test-Path secrets/admin_token)) {
  throw 'Secret initialization refused: one or more secret files already exist.'
}
$rng = [Security.Cryptography.RandomNumberGenerator]::Create()
$pageKey = New-Object byte[] 32
$adminToken = New-Object byte[] 32
$rng.GetBytes($pageKey)
$rng.GetBytes($adminToken)
$rng.Dispose()
[BitConverter]::ToString($pageKey).Replace('-', '').ToLowerInvariant() | Set-Content -Encoding ascii -NoNewline secrets/rnmdb_page_key
[BitConverter]::ToString($adminToken).Replace('-', '').ToLowerInvariant() | Set-Content -Encoding ascii -NoNewline secrets/admin_token
```

Never regenerate `rnmdb_page_key` while reusing the `rnmdb-data` volume. Back
up the database volume and its page-key file together. A lost page key cannot
decrypt the database; an intentional key rotation must use RNMDB's verified
offline rekey procedure while the application is stopped.

Build and start the service:

```powershell
docker compose up --build -d --wait --wait-timeout 120
```

The public catalog is available at `http://localhost:28740`. The administrator
listener is published to host loopback only at `http://127.0.0.1:28741` by
default, though peer containers on the Compose network can still reach its
container address. Override `PUBLIC_PORT`, `ADMIN_PORT`, or `ADMIN_HOST` in
`.env` when needed. Setting `ADMIN_HOST` to a non-loopback address exposes a
plaintext bearer-token API; only do so behind TLS and explicit network access
controls.

The `--wait` option requires Docker Compose 2.20 or newer. If startup fails,
inspect the health and logs before retrying:

```powershell
docker compose ps
docker compose logs --tail 200 app
```

## Run With Cargo

The same secret files can be used for a local process:

```powershell
$env:PUBLIC_BIND_ADDR = '127.0.0.1:28740'
$env:ADMIN_BIND_ADDR = '127.0.0.1:28741'
$env:RNMDB_PATH = "$PWD/data/aperip-nomos.rnmdb"
$env:RNMDB_PAGE_KEY_FILE = "$PWD/secrets/rnmdb_page_key"
$env:ADMIN_TOKEN_FILE = "$PWD/secrets/admin_token"
cargo run --locked
```

## API Example

```powershell
$token = (Get-Content -Raw secrets/admin_token).Trim()
$curlConfig = Join-Path secrets ("curl-{0}.cfg" -f [guid]::NewGuid())
$sampleLicense = Join-Path secrets ("license-{0}.txt" -f [guid]::NewGuid())
try {
  'Permission is hereby granted.' | Set-Content -Encoding ascii $sampleLicense
  $sampleLicenseForCurl = $sampleLicense.Replace('\', '/')
  @(
    'silent'
    'show-error'
    'fail'
    "header = `"Authorization: Bearer $token`""
    'form = "title=MIT License"'
    "form = `"file=@$sampleLicenseForCurl;type=text/plain`""
    'url = "http://127.0.0.1:28741/api/admin/licenses"'
  ) | Set-Content -Encoding ascii $curlConfig
  curl.exe --config $curlConfig
  curl.exe -fsS http://127.0.0.1:28740/api/licenses
  curl.exe -fsS http://127.0.0.1:28740/api/licenses/mit
  @(
    'silent'
    'show-error'
    'fail'
    'request = "DELETE"'
    "header = `"Authorization: Bearer $token`""
    'url = "http://127.0.0.1:28741/api/admin/licenses/mit"'
  ) | Set-Content -Encoding ascii $curlConfig
  curl.exe --config $curlConfig
} finally {
  Remove-Item -Force -ErrorAction SilentlyContinue $curlConfig, $sampleLicense
  $token = $null
}
```

## RNMDB Boundary

The service pins
[`czxieddan/RNovModularDB`](https://github.com/czxieddan/RNovModularDB)
revision `013ec2f48a1dab89997430d72c2b176be2c29d47`. One dedicated actor thread
owns the encrypted `LocalSession`; all reads and writes are serialized through
a bounded queue. Successful mutations commit RNMDB transactions before their
HTTP responses are returned.

## Name

`Nomos` is the ancient Greek word for law, custom, and shared order. The name
connects the archive's legal subject with Aperip's role as its curator.

# macOS release runbook

Flowmates releases are universal (`arm64` + `x86_64`), Developer ID signed,
notarized, and distributed as both a DMG and a signed Tauri updater archive.
The release workflow only creates a GitHub Release after every build and
verification gate succeeds.

## One-time Apple setup

Create a **Developer ID Application** certificate in the Apple Developer
account, export it with its private key as a password-protected `.p12`, and
base64-encode the file without changing its bytes.

Configure these GitHub Actions repository secrets:

| Secret | Purpose |
|---|---|
| `APPLE_CERTIFICATE` | Base64-encoded Developer ID `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | Password used when exporting the `.p12` |
| `KEYCHAIN_PASSWORD` | Random password for CI's temporary keychain |
| `APPLE_ID` | Apple account used for notarization |
| `APPLE_PASSWORD` | App-specific password for that Apple account |
| `APPLE_TEAM_ID` | Ten-character Apple Developer team ID |
| `TAURI_SIGNING_PRIVATE_KEY` | Private Tauri updater key or its complete contents |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Updater private-key password |

Keep the updater private key outside the repository and back it up. Losing it
prevents installed builds from trusting future updates. Its public counterpart
is the `plugins.updater.pubkey` value in `tauri.conf.json`.

Configure public build-time values as GitHub Actions repository **variables**,
not secrets:

- `NEXT_PUBLIC_SUPABASE_URL`
- `NEXT_PUBLIC_SUPABASE_ANON_KEY`
- `TAURI_JIRA_CLIENT_ID`
- `TAURI_LINEAR_CLIENT_ID`

OAuth client secrets are Supabase Edge Function secrets. They must never be
added to a desktop build job.

## Cut a release

1. Update the same semantic version in root `package.json`,
   `apps/agent/package.json`, `apps/agent/src-tauri/Cargo.toml`, and
   `apps/agent/src-tauri/tauri.conf.json`.
2. Ensure CI is green on `main` and test screen-recording consent, local model
   startup, cloud sign-in, sync, and update checks on both Mac architectures.
3. Create and push a signed tag such as `v3.6.0`.

```bash
git tag -s v3.6.0 -m "Flowmates 3.6.0"
git push origin v3.6.0
```

The workflow validates that the tag version matches all four manifests. It
then runs frontend/Rust gates, verifies model hashes, imports the certificate
into an ephemeral keychain, builds `universal-apple-darwin`, notarizes, and
checks both slices, the code signature, the stapled ticket, and Gatekeeper.
The checkout must contain `local_llm/bin/llama-server` with executable mode;
the workflow rejects a missing, modified, or single-architecture binary before
signing. It is declared through Tauri `externalBin`, so the bundler signs the
copied `Contents/MacOS/llama-server` before it signs and notarizes the outer app.

On success it publishes:

- `Flowmates-Agent_<version>_universal.dmg`
- `Flowmates-Agent_<version>_universal.app.tar.gz`
- the updater archive signature
- `latest.json`

Both `darwin-aarch64` and `darwin-x86_64` entries in `latest.json` point to the
same universal updater archive.

## Failure policy

Do not manually attach an artifact from a failed run to a release. Fix the
cause, delete the failed tag only if it has not been distributed, and cut a new
version/tag once the source is immutable. Rotate the Apple certificate or
updater key immediately if CI output or repository history ever exposes it.

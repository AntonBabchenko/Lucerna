# Security and Release Policy

Lucerna positions itself as a transparent alternative to launchers that ship adware, telemetry, or hidden processes. The claim "we have nothing to hide" only means something if it is verifiable. This document specifies the mechanics that make it verifiable.

For vulnerability disclosure, see the short [`SECURITY.md`](../SECURITY.md) at the repository root.

## Part A — Reproducible builds and supply chain

1. **Reproducible builds are a goal, not a release blocker.** The target: any third party can rebuild the binary from a tagged commit and get the same SHA256. Tauri has known sources of nondeterminism (embedded assets, bundle timestamps); these are documented as they are discovered and the gaps are closed over time. Until full reproducibility is achieved, the pipeline still ships — the gap is honest, not hidden.

2. **All releases come from GitHub Actions, never local machines.** The release workflow is public; its logs and its inputs (the tagged commit, the lockfile, the cached dependencies) are public. **Status: implemented in `.github/workflows/release.yml`** — it builds every pushed `v*` tag on a GitHub-hosted runner; `v0.9.0` will be the first release it produces. (The legacy `v0.1.0` tag predates this and was built locally.)

3. **Software Bill of Materials.** The release workflow runs `cargo-cyclonedx` to generate a CycloneDX SBOM and attaches it as a release asset. **Status: implemented in `release.yml`; first produced for the v0.9.0 release.**

4. **Advisory scanning in CI (the `cargo-audit` equivalent).** **Status: implemented** — the `cargo-deny` job in `.github/workflows/ci.yml` runs `cargo deny check`, which includes the `advisories` check against the RustSec advisory database (the same DB `cargo-audit` uses). It runs on every push and PR and is ungated, so a newly-published advisory can trip it even when `Cargo.lock` has not changed. A PR that introduces a vulnerable dependency is blocked.

5. **`cargo-deny` in CI.** **Status: implemented** in the `cargo-deny` CI job (`cargo deny check`, SHA-pinned toolchain, version-pinned `cargo-deny`). It enforces, per `deny.toml`:
   - License allowlist (FOSS only — no proprietary, no unknown).
   - Banned / duplicate crate policy.
   - Approved source registries only.
   - RustSec advisories (see item 4).

6. **`Cargo.lock` is committed.** This is a binary crate, so locked dependencies are part of the release contract.

7. **GitHub Actions are pinned by commit SHA**, not by tag (`uses: actions/checkout@<40-char-sha>`).

## Part B — Signing

1. **Starting position (implemented in `release.yml`, first applied to the v0.9.0 release):**
   - `SHA256SUMS` is published with every release.
   - `cosign` keyless signatures via sigstore are produced for every release artifact. The signer identity is the GitHub Actions OIDC token issued to this repo — verifiable by anyone against the public sigstore transparency log. No long-lived signing key is required.

2. **GPG signing is optional and added later** if a maintainer chooses to publish a long-lived key. When that happens, the public key goes into a `KEYS` file at the repository root.

3. **OS-level code signing when certificates exist:**
   - Windows: an EV or OV code-signing certificate (without one, SmartScreen flags unsigned binaries). Status: not acquired yet.
   - macOS: Developer ID certificate + Apple notarization. Status: not acquired yet.
   - Until certificates are acquired, binaries ship unsigned at the OS level. Release notes link to verification instructions for `cosign` and `SHA256SUMS`.

## Part C — Network audit

The product value "no hidden phone-home" is enforced in code, not merely displayed.

1. **The allowlist is enforced at the chokepoint.** Every outbound request goes through `network::request` / `network::download`, which reject any host not in `network::allowlist::ALLOWED_PATTERNS` before the request is sent — a request to a non-allowlisted host never leaves the process. `src-tauri/tests/structural_no_raw_http.rs` fails the build if an HTTP client is constructed outside `network::`.

2. **Wire-level self-audit (planned).** A CI integration test will boot the application in a controlled environment with a packet-capture tool, perform only "launch vanilla 1.20.x," and assert that every captured request targets an allowlisted host — an independent, out-of-process confirmation of the in-code enforcement. Status: not yet implemented; tracked in the project roadmap.

3. **Single source of truth for the allowlist.** The Rust constant `network::allowlist::ALLOWED_PATTERNS` is the source of truth. The table in `docs/PRINCIPLES.md` Part A mirrors it for human readers and is kept in sync by code review — there is no markdown-parsing build step (an earlier plan to add one was dropped as brittle).

4. **Content Security Policy applies only to production builds.** The CSP declared in `src-tauri/tauri.conf.json` (`default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https:; …`) is enforced on the `asset://` custom protocol used by `tauri build`. **It is NOT injected into `pnpm tauri dev`**, which loads `http://localhost:1420` via Vite without the CSP header. A developer adding a new image CDN host and testing only in dev mode will not see a CSP violation; the violation surfaces in production. Always smoke-test new external hosts against a `tauri build` artifact before submitting.

## Part D — Disclosure

The repository root contains a short [`SECURITY.md`](../SECURITY.md) with the contact channel for reporting vulnerabilities; this document references it.

- **Coordinated disclosure window:** 90 days by default. Expedited for actively exploited issues.
- **CVE assignment** is requested for confirmed vulnerabilities; credit is given to the reporter unless they request otherwise. Release notes for the fix mention the CVE ID.

## Part E — Bundled credentials

1. **The CurseForge API key is embedded in release binaries, by necessity.** CurseForge's Eternal API requires an `x-api-key` header on every request; there is no anonymous access. Lucerna's official release binaries carry a CurseForge **application** key, injected at compile time from a GitHub Actions secret (`CURSEFORGE_API_KEY`) in `release.yml` and **never committed to the repository**. This mirrors how open-source launchers such as Prism ship CurseForge support.

2. **This key is extractable, and we say so.** Unlike a server-side secret, a key compiled into a distributed binary can be recovered by anyone who inspects it. It is therefore treated as an *application identity*, not a user secret: it is rate-limited per application (shared across release users) and can be rotated by the maintainer (which requires a new release). A user who prefers their own key can enter one in Settings → Integrations; a personal key takes precedence over the embedded one, so a user is never forced onto the shared key and can self-heal if the embedded key is ever revoked.

3. **Self-built and forked binaries carry no key unless one is supplied.** Builds without the `LUCERNA_CURSEFORGE_API_KEY` env var set at compile time fall back to the manual key-entry flow — no key is hidden in source. See `CONTRIBUTING.md` for how a fork bakes in its own key.

## Part F — SFTP dependency and credential handling (own-server upload)

The "own server" feature (slice 3) introduces two new Rust crates: `russh` and `russh-sftp` (pure-Rust SSH/SFTP client; Apache-2.0; `cargo deny check` clean as of the crate versions pinned in `Cargo.lock`).

1. **Cryptography configuration.** `russh` is compiled with `default-features = false`, enabling only the `aws-lc-rs` and `flate2` features. This explicitly excludes the `rsa` feature, which would pull in the `rsa` crate affected by RUSTSEC-2023-0071 (Marvin timing side-channel). As a result, RSA host keys are unsupported; only ed25519 and ECDSA host keys are accepted.

2. **SFTP credential handling.** The user's SFTP password is stored in the OS keyring via the same keychain abstraction used for Microsoft OAuth tokens — never written to `server.json`, configuration files, or log output. Host identity is verified via TOFU (trust-on-first-use): the server's SHA-256 fingerprint is persisted on first connect; a changed fingerprint blocks subsequent uploads and prompts the user to re-confirm.

3. **Module isolation and structural guard.** All `russh`/`russh-sftp` usage is confined to `src-tauri/src/servers_runtime/transfer.rs`. The structural guard `src-tauri/tests/structural_no_raw_sftp.rs` fails the build if SSH/SFTP client construction appears anywhere else — mirroring the `structural_no_raw_http.rs` and `structural_no_raw_spawn.rs` guards. For the product rationale of this sanctioned outbound channel, see `docs/PRINCIPLES.md` Part A, commitment 3.

# Security and Release Policy

Lucerna positions itself as a transparent alternative to launchers that ship adware, telemetry, or hidden processes. The claim "we have nothing to hide" only means something if it is verifiable. This document specifies the mechanics that make it verifiable.

For vulnerability disclosure, see the short [`SECURITY.md`](../SECURITY.md) at the repository root.

## Part A — Reproducible builds and supply chain

1. **Reproducible builds are a goal, not a release blocker.** The target: any third party can rebuild the binary from a tagged commit and get the same SHA256. Tauri has known sources of nondeterminism (embedded assets, bundle timestamps); these are documented as they are discovered and the gaps are closed over time. Until full reproducibility is achieved, the pipeline still ships — the gap is honest, not hidden.

2. **All releases come from GitHub Actions, never local machines.** The release workflow is public; its logs and its inputs (the tagged commit, the lockfile, the cached dependencies) are public. **Status: implemented** — `.github/workflows/release.yml` builds every `v*` tag on a GitHub-hosted runner. (The legacy `v0.1.0` tag predates this and was built locally; `v0.9.0` is the first workflow-built release.)

3. **Software Bill of Materials.** `cargo-cyclonedx` generates a CycloneDX SBOM on every release, attached as a release asset. **Status: implemented** (see `release.yml`).

4. **`cargo-audit` in CI** *(planned)*. Builds should fail on known vulnerabilities; PRs that introduce a vulnerable dependency are blocked. **Status: not yet implemented** — not currently a CI job.

5. **`cargo-deny` in CI** *(planned, not yet implemented)*. Will enforce:
   - License allowlist (FOSS only — no proprietary, no unknown).
   - Banned crate list (none yet; added when needed).
   - No duplicate versions of the same crate.
   - Only approved source registries.

6. **`Cargo.lock` is committed.** This is a binary crate, so locked dependencies are part of the release contract.

7. **GitHub Actions are pinned by commit SHA**, not by tag (`uses: actions/checkout@<40-char-sha>`).

## Part B — Signing

1. **Starting position (implemented):**
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

# Security and Release Policy

FTlauncher positions itself as a transparent alternative to launchers that ship adware, telemetry, or hidden processes. The claim "we have nothing to hide" only means something if it is verifiable. This document specifies the mechanics that make it verifiable.

For vulnerability disclosure, see the short [`SECURITY.md`](../SECURITY.md) at the repository root.

## Part A — Reproducible builds and supply chain

1. **Reproducible builds are a goal, not a release blocker.** The target: any third party can rebuild the binary from a tagged commit and get the same SHA256. Tauri has known sources of nondeterminism (embedded assets, bundle timestamps); these are documented as they are discovered and the gaps are closed over time. Until full reproducibility is achieved, the pipeline still ships — the gap is honest, not hidden.

2. **All releases come from GitHub Actions, never local machines.** The release workflow is public; its logs are public; its inputs (the tagged commit, the lockfile, the cached dependencies) are public.

3. **Software Bill of Materials.** `cargo-cyclonedx` generates an SBOM on every release; the SBOM is attached as a release asset.

4. **`cargo-audit` in CI.** Builds fail on known vulnerabilities. PRs that introduce a vulnerable dependency are blocked.

5. **`cargo-deny` in CI.** Enforces:
   - License allowlist (FOSS only — no proprietary, no unknown).
   - Banned crate list (none yet; added when needed).
   - No duplicate versions of the same crate.
   - Only approved source registries.

6. **`Cargo.lock` is committed.** This is a binary crate, so locked dependencies are part of the release contract.

7. **GitHub Actions are pinned by commit SHA**, not by tag (`uses: actions/checkout@<40-char-sha>`).

## Part B — Signing

1. **Starting position:**
   - `SHA256SUMS` published with every release.
   - `cosign` keyless signature via sigstore on every release artifact. The signer identity is the GitHub Actions OIDC token issued to this repo — verifiable by anyone with the public sigstore transparency log. No long-lived signing key is required to start.

2. **GPG signing is optional and added later** if a maintainer chooses to publish a long-lived key. When that happens, the public key goes into a `KEYS` file at the repository root.

3. **OS-level code signing when certificates exist:**
   - Windows: an EV or OV code-signing certificate (without one, SmartScreen flags unsigned binaries). Status: not acquired yet.
   - macOS: Developer ID certificate + Apple notarization. Status: not acquired yet.
   - Until certificates are acquired, binaries ship unsigned at the OS level. Release notes link to verification instructions for `cosign` and `SHA256SUMS`.

## Part C — Network audit

The product value "no hidden phone-home" is mechanically enforced.

1. **Self-audit integration test** runs in CI on every release:
   - Launches the application in a controlled environment with a network capture tool (`mitmproxy` or equivalent).
   - Performs only the user action "launch vanilla 1.20.x" — no other clicks.
   - Asserts that no outbound request was made before the user action.
   - Asserts that every subsequent outbound request targets a host in the allowlist defined in `src-tauri/src/network/allowlist.rs`.
   - Test failure fails the release.

2. **Single source of truth for the allowlist.** The Rust constant in `src-tauri/src/network/allowlist.rs` and the table in `docs/PRINCIPLES.md` (Part A, item 2) must match. A separate CI test parses both and asserts equality. Drift between the two is a build failure.

## Part D — Disclosure

The repository root contains a short [`SECURITY.md`](../SECURITY.md) with the contact channel for reporting vulnerabilities; this document references it.

- **Coordinated disclosure window:** 90 days by default. Expedited for actively exploited issues.
- **CVE assignment** is requested for confirmed vulnerabilities; credit is given to the reporter unless they request otherwise. Release notes for the fix mention the CVE ID.

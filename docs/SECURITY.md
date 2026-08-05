# Security and Release Policy

Lucerna positions itself as a transparent alternative to launchers that ship adware, telemetry, or hidden processes. The claim "we have nothing to hide" only means something if it is verifiable. This document specifies the mechanics that make it verifiable.

For vulnerability disclosure, see the short [`SECURITY.md`](../SECURITY.md) at the repository root.

## Part A — Reproducible builds and supply chain

1. **Reproducible builds are a goal, not a release blocker.** The target: any third party can rebuild the binary from a tagged commit and get the same SHA256. Tauri has known sources of nondeterminism (embedded assets, bundle timestamps); these are documented as they are discovered and the gaps are closed over time. Until full reproducibility is achieved, the pipeline still ships — the gap is honest, not hidden.

2. **All releases come from GitHub Actions, never local machines.** The release workflow is public; its logs and its inputs (the tagged commit, the lockfile, the cached dependencies) are public. **Status: implemented in `.github/workflows/release.yml`** — it builds every pushed `v*` tag on a GitHub-hosted runner, and has done so since `v0.9.0`. (The legacy `v0.1.0` tag predates this and was built locally.)

3. **Software Bill of Materials.** The release workflow runs `cargo-cyclonedx` to generate a CycloneDX SBOM and attaches it as a release asset. **Status: implemented in `release.yml`; produced for every release since v0.9.0.**

4. **Advisory scanning in CI (the `cargo-audit` equivalent).** **Status: implemented** — the `cargo-deny` job in `.github/workflows/ci.yml` runs `cargo deny check`, which includes the `advisories` check against the RustSec advisory database (the same DB `cargo-audit` uses). It runs on every push and PR and is ungated, so a newly-published advisory can trip it even when `Cargo.lock` has not changed. A PR that introduces a vulnerable dependency is blocked.

5. **`cargo-deny` in CI.** **Status: implemented** in the `cargo-deny` CI job (`cargo deny check`, SHA-pinned toolchain, version-pinned `cargo-deny`). It enforces, per `deny.toml`:
   - License allowlist (FOSS only — no proprietary, no unknown).
   - Banned / duplicate crate policy.
   - Approved source registries only.
   - RustSec advisories (see item 4).

6. **`Cargo.lock` is committed.** This is a binary crate, so locked dependencies are part of the release contract.

7. **GitHub Actions are pinned by commit SHA**, not by tag (`uses: actions/checkout@<40-char-sha>`).

## Part B — Signing

1. **Starting position (implemented in `release.yml`, applied to every release since v0.9.0):**
   - `SHA256SUMS` is published with every release.
   - `cosign` keyless signatures via sigstore are produced for every release artifact. The signer identity is the GitHub Actions OIDC token issued to this repo — verifiable by anyone against the public sigstore transparency log. No long-lived signing key is required.

2. **GPG signing is optional and added later** if a maintainer chooses to publish a long-lived key. When that happens, the public key goes into a `KEYS` file at the repository root.

3. **OS-level code signing when certificates exist:**
   - Windows: an EV or OV code-signing certificate (without one, SmartScreen flags unsigned binaries). Status: not acquired yet.
   - macOS: Developer ID certificate + Apple notarization. Status: not acquired yet.
   - Linux: no OS-level signing scheme applies to the `.AppImage` / `.deb` / `.rpm` artifacts we publish; integrity rests on cosign + `SHA256SUMS`.
   - Until those certificates are acquired, Windows binaries ship unsigned at the OS level and SmartScreen warns on first run. macOS builds are **ad-hoc signed** (`codesign -s -`, asserted by a CI guard in `release.yml`) — enough to run, but not notarized, so Gatekeeper quarantines a downloaded copy until the user clears it with `xattr -dr com.apple.quarantine /Applications/Lucerna.app`. Every artifact on every platform is cosign-signed regardless; release notes link to verification instructions for `cosign` and `SHA256SUMS`.

## Part C — Network audit

The product value "no hidden phone-home" is enforced in code, not merely displayed.

1. **The allowlist is enforced at the chokepoint.** Every outbound request goes through `network::request` / `network::download`, which reject any host not in `network::allowlist::ALLOWED_PATTERNS` before the request is sent — a request to a non-allowlisted host never leaves the process. `src-tauri/tests/structural_no_raw_http.rs` fails the build if an HTTP client is constructed outside `network::`.

2. **Wire-level self-audit (planned).** A CI integration test will boot the application in a controlled environment with a packet-capture tool, perform only "launch vanilla 1.20.x," and assert that every captured request targets an allowlisted host — an independent, out-of-process confirmation of the in-code enforcement. Status: not yet implemented; tracked in the project roadmap.

3. **First channel the allowlist cannot cover — the consented dial.** The opt-in saved-server status feature must reach a host the *user* typed, which by definition cannot be allowlisted. It is therefore confined to `src-tauri/src/network/consent.rs`, behind an opaque `ConsentedTcp` whose socket is private, and guarded by `src-tauri/tests/structural_consented_dial.rs` (no `TcpStream` outside that file, no `UdpSocket` anywhere, consent check present in `open`). The permission defaults to off and is re-read from disk on every dial — there is no cached consent, so revoking it takes effect at once. Concurrency is capped at 4 process-wide, and every stage is timeout- and size-bounded (3 s connect, 5 s exchange, 256 KiB response). Only addresses already present in that instance's `servers.dat` are dialed, only while a server list is on screen (closing it stops the sweep), and their addresses are kept out of the launcher log. One scope caveat, surfaced by the feature's own security review: a modpack's `overrides/` may ship `servers.dat` entries — a legitimate, widely used practice that is therefore not blocked — so a pack author can end up choosing dial targets in a list the user opted to check. There is no feedback channel to the pack (results stay in the UI), and joining such a server exposes the same IP anyway, so this is handled by accurate UI copy rather than by narrowing the feature. For the product rationale see `docs/PRINCIPLES.md` Part A, commitment 4.

4. **Second channel the allowlist cannot cover — the loopback seam.** The `Local` AI translation provider posts to a model server on `127.0.0.1` at a user-supplied port. Putting `127.0.0.1` in `ALLOWED_PATTERNS` would hand every code path in the launcher access to every local port, so this gets its own seam instead: `src-tauri/src/network/loopback.rs`, where the host is a compile-time constant and only the port and path come from the caller. `src-tauri/tests/structural_loopback_confined.rs` fails the build if anything outside `l10n::prefill` calls it. Confinement alone is not the whole guarantee — every function in `l10n::prefill::provider` that reaches a model also requires a `network::consent::AiConsent`, whose field is private to `network::consent`, so holding one is proof the permission check ran. The seam uses the generation HTTP client (no read timeout: a local CPU model can take minutes to first token), bounded by the caller's total timeout. Unlike the consented dial, this channel sends nothing off the machine. For the product rationale see `docs/PRINCIPLES.md` Part A, commitment 5.

5. **Single source of truth for the allowlist.** The Rust constant `network::allowlist::ALLOWED_PATTERNS` is the source of truth. The table in `docs/PRINCIPLES.md` Part A mirrors it for human readers and is kept in sync by code review — there is no markdown-parsing build step (an earlier plan to add one was dropped as brittle).

6. **Content Security Policy applies only to production builds.** The CSP declared in `src-tauri/tauri.conf.json` (`default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https:; …`) is enforced on the `asset://` custom protocol used by `tauri build`. **It is NOT injected into `pnpm tauri dev`**, which loads `http://localhost:1420` via Vite without the CSP header. A developer adding a new image CDN host and testing only in dev mode will not see a CSP violation; the violation surfaces in production. Always smoke-test new external hosts against a `tauri build` artifact before submitting.

7. **`LUCERNA_*` environment overrides stay live in release builds — deliberately.** The test-seam module (`src-tauri/src/test_seam.rs`) resolves a small set of `LUCERNA_*` environment variables (e.g. `LUCERNA_EXTRA_ALLOWED_HOSTS`, per-endpoint URL overrides for the auth chain and loader metadata) in production as well as under test, so an operator can point a release binary at a mirror or staging endpoint. This is an explicit, documented trade-off: anyone who can set environment variables for the process can already modify the binary or its config, so the overrides add no privilege an attacker doesn't have — but they DO widen what a mis-configured environment can change (including extending the network allowlist and redirecting the Microsoft/XSTS auth endpoints). The full override list lives in `test_seam.rs`; treat additions to it as security-relevant and review them against this section.

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

## Part G — Desktop integration: launch arguments, `lucerna://` links, and shortcuts

Desktop integration adds two ways for something outside the launcher to ask it to do something. They are deliberately **not** equally trusted, and the whole design follows from that split.

1. **Launch arguments are trusted; `lucerna://` links are not.** Anything able to pass argv to `lucerna.exe` can already start arbitrary programs on that machine, so `--launch <instance> [--world … | --server …]` may start the game with no prompt — that is what a desktop shortcut uses. A URL scheme, by contrast, is reachable from any web page with a single navigation. Therefore **no `lucerna://` URL can launch anything**. This is also why desktop shortcuts are real `.lnk`/`.desktop` files carrying argv rather than `.url` files carrying a scheme URL, even though the latter would have been less code.

   Two independent gates enforce it, because one is not enough:

   - **At the argument parser (`src-tauri/src/cli.rs`).** Windows substitutes the URL into the registered `"<exe>" "%1"` command *textually*, so a URL containing a double quote closes that quoting early and its remainder arrives as extra argv tokens — the classic URI-handler argument-injection class (the same shape as CVE-2020-6109 and its many siblings). A hostile page opening `lucerna://x?a=1" --launch "victim` therefore reaches the process as `[exe, lucerna://x?a=1, --launch, victim]`. `cli::parse` looks for the scheme **first**: if any argument begins with `lucerna:` (the bare scheme, not just `lucerna://` — the shell dispatches the slash-less form too), the entire command line is treated as an untrusted URL activation and can only produce "open the import dialog", with every flag beside it ignored. The injected `--launch` is inert. Pinned by `a_quote_breakout_in_a_url_cannot_inject_a_launch`.
   - **At the link grammar (`src-tauri/src/deeplink.rs`).** The grammar has no representation for a launch at all, so even a URL that reached the resolver could not express one. Pinned by `no_link_can_ask_for_a_launch`.

2. **A link never installs anything.** An inbound link resolves to *pack metadata only* — one GET to an already-allowlisted API host (`api.modrinth.com` / `api.curseforge.com`) — and then opens the same modpack detail modal and import picker the Browse tab uses. The user still chooses a version and confirms the file selection before a single pack byte is downloaded. When the link came from another application rather than the user's own paste, the dialog says so.

3. **Inbound links are validated before they reach any API call.** `parse_import_url` enforces: length ≤ 2048; no control characters or whitespace; scheme exactly `lucerna` or `https` (plain `http` is rejected, so a link cannot smuggle in a downgrade); exact-match host allowlist for page URLs (`modrinth.com`, `www.modrinth.com`, `curseforge.com`, `www.curseforge.com` — a look-alike such as `evilmodrinth.com` is rejected); and `[A-Za-z0-9_.-]{1,64}` for every project and version reference, which rejects path traversal and encoded separators in one rule. Download hosts are unchanged: pack files still flow through `network::` and its allowlist, and this feature adds no allowlist entry.

4. **Scheme registration is opt-in and reversible.** Nothing is written to the OS until the user turns it on in Settings → Integrations, where the exact key is shown next to the toggle: `HKCU\Software\Classes\lucerna` (per-user, no elevation, matching the current-user install mode). Turning the toggle off deletes the key. With the setting off the launcher performs no registry access for this feature at all. If the setting is on and the recorded command points at a different executable — a moved install, an update, a portable copy — the app re-asserts the key at startup so links do not open a binary that is no longer there. Registration is confined to `platform::protocol`; the structural guard `src-tauri/tests/structural_platform_chokepoint.rs` fails the build if a registry-mutating call appears outside `src/platform/`.

5. **Scope.** Scheme registration is Windows-only in this version; Linux (a `.desktop` handler plus a MIME-database refresh, varying by desktop environment) and macOS (`CFBundleURLTypes`) report as unsupported rather than shipping untested OS integration. `curseforge://` and `modrinth://` are deliberately **not** claimed — those schemes belong to those vendors' own applications.

6. **Shortcut contents are validated and escaped at creation time.** A shortcut's world folder and server address pass through the same validators the launch path uses (`worlds::fs::validate_segment`, `launch::quick_play::validate_server_address`) before the file is written, so a shortcut cannot encode an argument the launcher would refuse on double-click. Those validators reject whitespace and control characters but **not** quotes, so the argument encoder does not assume clean input: it follows the `CommandLineToArgvW` rules (quote the token, escape embedded `"` as `\"`, double backslash runs before a quote). Without that, a saved server address containing a quote would close its token early and the remainder would re-split into extra argv entries, launching with a corrupted target. Two tests pin it: one round-trips the argument vector through the parser, and one round-trips the *actual command-line string* through an OS-rule splitter and back into the parser.

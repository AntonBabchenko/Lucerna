# Lucerna Privacy Policy

_Last updated: 2026-07-02_

## 1. Summary

Lucerna does not transmit anything to Lucerna servers — there
are no such servers. All launcher state lives on your machine.
Third-party APIs are called only for documented purposes: the Mojang
version manifest and downloads, Microsoft / Xbox Live sign-in (when
you choose to use it), Modrinth and CurseForge mod and modpack
browsing, optional log sharing via mclo.gs (only when you click
Share), and an optional GitHub Releases check for launcher updates.

## 2. What data is stored on your machine

- `account.json` in `%APPDATA%/com.lucerna.app/`: a list of
  accounts with `{id, kind, name, uuid, expires_at}` per entry.
  Microsoft accounts additionally have a refresh token and a
  Minecraft access token in the OS keyring (Windows Credential
  Manager via the `keyring` crate), keyed by the account's local
  `id`. Tokens are never written to disk.
- Per-instance Minecraft state in
  `%APPDATA%/com.lucerna.app/instances/<instance-id>/.minecraft/`:
  worlds, screenshots, mods, configs, logs — the same shape as the
  Mojang reference launcher's `.minecraft` folder, but isolated to
  one instance.
- Mod and modpack download caches.
- A few recent session logs from the launcher itself.

Nothing in the list above leaves your machine unless you explicitly
trigger a share action.

## 3. What data leaves your machine, and where it goes

Every outbound HTTP request is checked against a static host
allowlist in `network::allowlist::ALLOWED_PATTERNS` (see
[`src-tauri/src/network/allowlist.rs`](src-tauri/src/network/allowlist.rs)).
A request to any host not on this list is refused before it's sent.
The list is mirrored in
[`docs/PRINCIPLES.md`](docs/PRINCIPLES.md) Part A item #2.

- **Mojang.** `*.mojang.com`, `*.minecraft.net`,
  `piston-meta.mojang.com`, `piston-data.mojang.com` — version
  manifest, libraries, assets, the JRE manifest, and the Minecraft
  client jar.
- **Microsoft sign-in chain.** `login.microsoftonline.com`,
  `login.live.com`, `user.auth.xboxlive.com`,
  `xsts.auth.xboxlive.com`, `api.minecraftservices.com` — only when
  you click "Sign in with Microsoft".
- **Modrinth.** `api.modrinth.com`, `cdn.modrinth.com` — mod /
  modpack browse and download.
- **CurseForge.** `api.curseforge.com`, `edge.forgecdn.net`,
  `mediafilez.forgecdn.net` — same purpose. Uses a CurseForge API
  key: release builds embed one at compile time so most users need
  nothing, but you can supply your own key (stored in the OS keyring),
  which takes precedence when present.
- **FTB (Feed The Beast) modpacks.** `api.modpacks.ch` (pack
  metadata), `dist.modpacks.ch` (pack file downloads) — only when you
  browse or install an FTB modpack. CurseForge-referenced files in an
  FTB pack reuse the forgecdn hosts above.
- **ATLauncher modpacks.** `api.atlauncher.com` (pack catalogue
  metadata), `download.nodecdn.net` (pack `Configs.json` manifest and
  mod file downloads) — only when you browse or install an ATLauncher
  modpack.
- **Mod loader meta and mavens.** `meta.fabricmc.net`,
  `maven.fabricmc.net`, `meta.quiltmc.org`, `maven.quiltmc.org`,
  `maven.minecraftforge.net`, `files.minecraftforge.net`,
  `maven.neoforged.net` — only when you install a loader.
- **GitHub.** `api.github.com` — release lookup, only when the
  launcher checks for an update. `github.com` — the release-asset
  download (installer / `SHA256SUMS` / cosign bundle) when you click
  Update; it redirects to a GitHub CDN, and update integrity rests on
  cosign + SHA-256 verification of the bytes, not the transport host.
- **Public-IP echo.** `api.ipify.org` — returns only your public IP
  address as plain text (no cookies, no request body). Called only
  when you open the "own server" hosting view and ask for your public
  address to set up port forwarding; never automatic.
- **mclo.gs paste service.** `api.mclo.gs`, `mclo.gs` — only when
  you click Share in the Logs viewer. The shared log is anonymised
  (Windows user-path scrubbing) and shown to you before upload.

Beyond this HTTP allowlist there is one further, user-initiated
outbound channel: **SFTP upload** to a host **you provide**. It is used
only by the "own server" feature to transfer your assembled server
archive to your own machine — never a Lucerna-chosen endpoint, and
never any form of telemetry. The destination host and credentials are
entered by you; the SFTP password is stored in the OS keyring (never in
config files or logs), and the server's SSH fingerprint is remembered on
first connect (trust-on-first-use) so a changed fingerprint blocks the
upload until you re-confirm. It runs only when you explicitly start an
upload.

## 4. What we do not collect

- No telemetry SDK in the dependency tree (no PostHog, Sentry,
  Mixpanel, Amplitude, Google Analytics, Plausible, Umami, etc.).
- No advertising, no fingerprinting, no usage analytics, no
  crash-reporting service.
- No background "ping home" — the launcher does not reach out
  unless you navigate to a feature that calls one of the
  allowlisted hosts above.

These guarantees are enforced two ways: a lint job in CI
(`tools/check-no-network-calls.mjs`) refuses commits that introduce
forbidden network APIs in the frontend; build-failing structural
tests (`tests/structural_no_raw_http.rs`,
`tests/structural_no_raw_spawn.rs`) refuse any HTTP client
construction or subprocess spawn outside the documented chokepoint
modules.

## 5. How long we keep data

- Microsoft refresh tokens and MC access tokens: until you sign out
  of that account or remove it. Both actions clear the keyring
  entries best-effort.
- Local account state, instance files, mod caches: until you delete
  the instance or uninstall the launcher.
- Anonymised mclo.gs paste: retention is controlled by mclo.gs (see
  https://mclo.gs/about). Lucerna only keeps the resulting URL
  in the launcher's local log.

## 6. Your choices

- You can use offline-only accounts (no Microsoft sign-in
  required) for LAN and single-player.
- To erase all Lucerna data: uninstall the launcher, delete
  `%APPDATA%/com.lucerna.app/`, and remove the relevant entries
  from Windows Credential Manager (Control Panel → Credential
  Manager → Generic Credentials → look for these entries:
  the CurseForge API key (network address `lucerna`, username
  `curseforge-api-key`), the Microsoft refresh token (network
  address `lucerna-microsoft-refresh`, username `<account-id>`),
  the Minecraft access token (network address
  `lucerna-mc-access`, username `<account-id>`), and — if you have
  configured an "own server" SFTP upload — the SFTP password
  (network address `lucerna-sftp-password`, username
  `<server-id>`). The exact rendering varies by OS; macOS Keychain
  shows them under the same service names with the account set to
  `<account-id>` / `<server-id>`, and Linux Secret Service stores them
  as schema attributes on the same service).

## 7. Children's privacy

Lucerna does not knowingly request or process data from
children under 13. Microsoft Xbox Live sign-in enforces its own
age-verification flow; XSTS XErr `2148916233` ("child account
requires parental consent") is surfaced by the launcher as a typed
error with a user-facing message.

## 8. Changes to this policy

Changes are tracked via the file's git history at
https://github.com/AntonBabchenko/Lucerna/commits/main/PRIVACY.md.
Material changes are also called out in release notes.

## 9. Contact

For privacy questions, email **anton.babchenko@outlook.com**. For
non-privacy issues, use
[GitHub Issues](https://github.com/AntonBabchenko/Lucerna/issues).

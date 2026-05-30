# FTlauncher Privacy Policy

_Last updated: 2026-05-27_

## 1. Summary

FTlauncher does not transmit anything to FTlauncher servers — there
are no such servers. All launcher state lives on your machine.
Third-party APIs are called only for documented purposes: the Mojang
version manifest and downloads, Microsoft / Xbox Live sign-in (when
you choose to use it), Modrinth and CurseForge mod and modpack
browsing, optional log sharing via mclo.gs (only when you click
Share), and an optional GitHub Releases check for launcher updates.

## 2. What data is stored on your machine

- `account.json` in `%APPDATA%/com.ftlauncher.app/`: a list of
  accounts with `{id, kind, name, uuid, expires_at}` per entry.
  Microsoft accounts additionally have a refresh token and a
  Minecraft access token in the OS keyring (Windows Credential
  Manager via the `keyring` crate), keyed by the account's local
  `id`. Tokens are never written to disk.
- Per-instance Minecraft state in
  `%APPDATA%/com.ftlauncher.app/instances/<instance-id>/.minecraft/`:
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
  `mediafilez.forgecdn.net` — same purpose. Requires a user-supplied
  CurseForge API key, stored in the OS keyring.
- **Mod loader meta and mavens.** `meta.fabricmc.net`,
  `maven.fabricmc.net`, `meta.quiltmc.org`, `maven.quiltmc.org`,
  `maven.minecraftforge.net`, `files.minecraftforge.net`,
  `maven.neoforged.net` — only when you install a loader.
- **GitHub.** `api.github.com` — release lookup, only when the
  launcher checks for an update.
- **mclo.gs paste service.** `api.mclo.gs`, `mclo.gs` — only when
  you click Share in the Logs viewer. The shared log is anonymised
  (Windows user-path scrubbing) and shown to you before upload.

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
  https://mclo.gs/about). FTlauncher only keeps the resulting URL
  in the launcher's local log.

## 6. Your choices

- You can use offline-only accounts (no Microsoft sign-in
  required) for LAN and single-player.
- To erase all FTlauncher data: uninstall the launcher, delete
  `%APPDATA%/com.ftlauncher.app/`, and remove the relevant entries
  from Windows Credential Manager (Control Panel → Credential
  Manager → Generic Credentials → look for entries whose Internet
  or network address is `ftlauncher` — the username field will be
  one of `curseforge-api-key`, `microsoft-refresh-<account-id>`,
  or `microsoft-mc-access-<account-id>`. The exact rendering varies
  by OS; macOS Keychain shows them under service "ftlauncher" with
  the same account names, and Linux Secret Service stores them as
  schema attributes on the same SERVICE).

## 7. Children's privacy

FTlauncher does not knowingly request or process data from
children under 13. Microsoft Xbox Live sign-in enforces its own
age-verification flow; XSTS XErr `2148916233` ("child account
requires parental consent") is surfaced by the launcher as a typed
error with a user-facing message.

## 8. Changes to this policy

Changes are tracked via the file's git history at
https://github.com/AntonBabchenko/FTlauncher/commits/main/PRIVACY.md.
Material changes are also called out in release notes.

## 9. Contact

For privacy questions, email **anton.babchenko@outlook.com**. For
non-privacy issues, use
[GitHub Issues](https://github.com/AntonBabchenko/FTlauncher/issues).

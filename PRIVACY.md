# Lucerna Privacy Policy

_Last updated: 2026-08-05_

## 1. Summary

Lucerna does not transmit anything to Lucerna servers — there
are no such servers. All launcher state lives on your machine.
Third-party APIs are called only for documented purposes: the Mojang
version manifest and downloads, Microsoft / Xbox Live sign-in and
profile changes (when you choose to use them), mod, modpack and
datapack browsing (Modrinth, CurseForge, FTB, ATLauncher, Vanilla
Tweaks), server cores and plugins (Paper, Purpur, Hangar), optional
log sharing via mclo.gs (only when you click Share), an optional
GitHub Releases check for launcher updates, and — only if you turn it
on and supply your own API key — an AI provider that drafts mod
translations for you. Section 3 lists every host individually.

## 2. What data is stored on your machine

Everything below lives under the launcher's **data root**. On a fresh
install that root is a `LucernaData` folder next to the Lucerna
executable (`%LOCALAPPDATA%/Lucerna/LucernaData` for a default
install); if that location is not writable, or if the install predates
this scheme, the root is `%APPDATA%/com.lucerna.app/` instead. Either
way you can move it anywhere from Settings → Storage, and a small
`data-location.json` pointer stays in `%APPDATA%/com.lucerna.app/` so
the launcher can find a relocated root.

- `account.json` in the data root: a list of
  accounts with `{id, kind, name, uuid, expires_at}` per entry.
  Microsoft accounts additionally have a refresh token and a
  Minecraft access token in the OS keyring (Windows Credential
  Manager via the `keyring` crate), keyed by the account's local
  `id`. Tokens are never written to disk.
- Per-instance Minecraft state in
  `<data root>/instances/<instance-id>/.minecraft/`:
  worlds, screenshots, mods, configs, logs — the same shape as the
  Mojang reference launcher's `.minecraft` folder, but isolated to
  one instance.
- Mod and modpack download caches.
- Cached skin and cape images (`skins/`, `capes/`) for the accounts
  you have signed in with, so the account panel and 3D preview do
  not re-download them each time.
- Per-server state in `servers/<server-id>/`, including
  `server.json`. If you have set up an "own server" SFTP upload,
  that file holds the host, port, and username you entered — the
  password goes to the OS keyring, never to disk.
- Custom instance icons you have uploaded.
- Mod translations you wrote yourself, stored per mod so one fix
  applies to every instance using that mod, plus the resource pack
  Lucerna builds from them.
- If you turned on AI translation drafting, the API key you entered
  goes to the OS keyring (never to disk); the provider you picked and
  the model name are ordinary settings.
- Installers downloaded by the in-app updater (`updates/`).
- A few recent session logs from the launcher itself, and a
  per-instance activity journal recording what the launcher did to
  that instance (installs, updates, repairs, launch outcomes).

Nothing in the list above leaves your machine unless you explicitly
trigger a share action.

## 3. What data leaves your machine, and where it goes

Every outbound HTTP request is checked against a static host
allowlist in `network::allowlist::ALLOWED_PATTERNS` (see
[`src-tauri/src/network/allowlist.rs`](src-tauri/src/network/allowlist.rs)).
A request to any host not on this list is refused before it's sent.
The list is mirrored in
[`docs/PRINCIPLES.md`](docs/PRINCIPLES.md) Part A item #2.

One further destination exists only if you switch it on: **your own
saved multiplayer servers.** With "Show status for my saved servers"
enabled (Settings → Game — off by default), Lucerna asks the servers
in that instance's list for their player count and version while the
list is open on screen; closing the list stops it. Those servers see
your IP address, exactly as they do when you join them. Note that a
modpack can ship server entries of its own (many do, to point at their
own server), so the list is not necessarily one you typed by hand — the
setting covers the list as it stands, and the in-app text says so. Nothing about them is stored or forwarded
anywhere, and with the setting off no packet is sent to them at all —
that is enforced in code (`network::consent`) and guarded by a
structural test, not just promised. See
[`docs/PRINCIPLES.md`](docs/PRINCIPLES.md) Part A commitment 4.

One documented exception: the `LUCERNA_EXTRA_ALLOWED_HOSTS`
environment variable adds patterns to this list at runtime. It exists
so integration tests can point the launcher at a local mock server,
and it is empty unless someone running the launcher sets it
deliberately. It is described in
[`docs/SECURITY.md`](docs/SECURITY.md) Part C.

- **Mojang.** `*.mojang.com`, `*.minecraft.net`,
  `piston-meta.mojang.com`, `piston-data.mojang.com` — version
  manifest, libraries, assets, the JRE manifest, and the Minecraft
  client jar. `textures.minecraft.net` also serves the skin and cape
  images shown in the account panel and the 3D preview; those are
  cached locally so they aren't re-fetched every time.
- **Microsoft sign-in chain.** `login.microsoftonline.com`,
  `login.live.com`, `user.auth.xboxlive.com`,
  `xsts.auth.xboxlive.com`, `api.minecraftservices.com` — when you
  click "Sign in with Microsoft".
- **Microsoft profile changes.** `api.minecraftservices.com` is also
  used when you change your appearance: selecting a cape sends the
  cape id, and uploading a skin **sends the image file itself** to
  Microsoft. Both happen only on your explicit action in the Skins
  and capes panel, and both go to the same Minecraft Services
  account API that owns your profile. Editing a skin locally sends
  nothing — only pressing Upload does.
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
- **Vanilla Tweaks.** `vanillatweaks.net` — the per-Minecraft-version
  datapack catalogue, and the zip the site builds from the packs you
  ticked. Requested only when you open the Vanilla Tweaks builder or
  check those packs for updates. The request carries the pack names
  you selected and nothing else.
- **Mod loader meta and mavens.** `meta.fabricmc.net`,
  `maven.fabricmc.net`, `meta.quiltmc.org`, `maven.quiltmc.org`,
  `maven.minecraftforge.net`, `files.minecraftforge.net`,
  `maven.neoforged.net` — only when you install a loader.
- **GitHub.** `api.github.com` — release lookup, only when the
  launcher checks for an update. `github.com` — the release-asset
  download (installer / `SHA256SUMS` / cosign bundle) when you click
  Update; it redirects to a GitHub CDN, and update integrity rests on
  cosign + SHA-256 verification of the bytes, not the transport host.
- **Paper and Purpur server cores.** `fill.papermc.io`,
  `fill-data.papermc.io` (Paper build metadata and the jar CDN) and
  `api.purpurmc.org` (Purpur builds) — only when you create or update
  a server on one of those cores.
- **Hangar plugin repository.** `hangar.papermc.io`,
  `hangarcdn.papermc.io` — plugin search, version listings, and the
  plugin files themselves. Requested when you open the plugin browser
  for a server, not on launcher start.
- **Public-IP echo.** `api.ipify.org` — returns only your public IP
  address as plain text (no cookies, no request body). Called only
  when you open the "own server" hosting view and ask for your public
  address to set up port forwarding; never automatic.
- **mclo.gs paste service.** `api.mclo.gs`, `mclo.gs` — only when
  you click Share in the Logs viewer. Before upload the log is
  scrubbed of Windows, macOS and Linux user paths, access tokens,
  session identifiers, and private LAN IP addresses, and the result
  is shown to you in full first.
- **AI translation drafting.** `api.anthropic.com`,
  `generativelanguage.googleapis.com`, `api.groq.com` — off by
  default. If you turn it on in Settings and enter your own API key,
  pressing "Translate with AI" sends **the English source strings of
  the mods you asked about**, plus your key, to the chat-completion
  endpoint of the one provider you selected; the other two are never
  contacted. Nothing else about you, your instances or your machine
  is included. The provider's own privacy policy and retention rules
  apply to what you send — Lucerna cannot make promises on their
  behalf. You can also point this at a model running on your own
  machine (see below), in which case nothing leaves the machine at
  all. Editing translations by hand never contacts anyone.

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

There is also one destination that is not on the internet at all: if
you choose **Local** as the AI translation provider, Lucerna posts the
strings to `127.0.0.1` on the port you entered — a model server
running on your own machine. Nothing leaves the computer. The host is
a compile-time constant (only the port is yours to choose), the code
path is confined to a single module (`network::loopback`) that only
the translation feature may call, and a structural test fails the
build if anything else calls it — so this cannot become a general
"talk to any local port" capability.

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
tests (`src-tauri/tests/structural_no_raw_http.rs`,
`src-tauri/tests/structural_no_raw_spawn.rs`) refuse any HTTP client
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
- Strings sent to an AI provider for translation drafting: retention
  is controlled by that provider under its own policy, not by
  Lucerna. Locally, only the resulting translations are kept — as
  ordinary files in your data root, until you delete them.

## 6. Your choices

- You can use offline-only accounts (no Microsoft sign-in
  required) for LAN and single-player.
- To erase all Lucerna data on Windows, the uninstaller does it for
  you: it lists every folder it found — the data root wherever it
  currently is, launcher settings and logs, browser cache, leftovers
  of older installers — with its path and size, plus how many saved
  sign-ins sit in Windows Credential Manager, and asks once whether
  to erase it all. Keeping your data is the default answer; agreeing
  also clears the credential-manager entries. A data root that is
  unreachable at uninstall time (an unplugged drive, say) is never
  deleted.
- To do it by hand instead — or on Linux and macOS — delete the data
  root (`LucernaData` next to the executable, or
  `%APPDATA%/com.lucerna.app/`, or wherever you moved it from
  Settings → Storage; the default folder keeps only a small pointer
  file once the root has been relocated), then remove the relevant
  entries from Windows Credential Manager (Control Panel → Credential
  Manager → Generic Credentials → look for these entries:
  the CurseForge API key (network address `lucerna`, username
  `curseforge-api-key`), the Microsoft refresh token (network
  address `lucerna-microsoft-refresh`, username `<account-id>`),
  the Minecraft access token (network address
  `lucerna-mc-access`, username `<account-id>`), the AI provider API
  key if you turned drafting on (network address
  `lucerna-ai-api-key`, username `anthropic` / `gemini` / `groq`),
  and — if you have
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

# Lucerna

[![CI](https://github.com/AntonBabchenko/Lucerna/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/AntonBabchenko/Lucerna/actions/workflows/ci.yml?query=branch%3Amain)

> **NOT AN OFFICIAL MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT.**

**Lucerna** — a clean, transparent open-source Minecraft launcher for
Windows. No telemetry, no ad injection, no hidden processes, no bundled adware.
The launcher itself is open-source under GPL-3.0-or-later; the Java runtime and Minecraft
files come straight from Mojang and are never modified.

> **Lucerna** /luˈkɛr.na/ — Latin for "lamp / light-bearer".

Lucerna integrates the official Modrinth, CurseForge, FTB, ATLauncher, and Hangar
APIs for browsing mods, resource packs, shaders, modpacks, and server plugins.
It supports Fabric / Quilt / Forge / NeoForge, isolates every Minecraft install
into its own instance, runs several instances at once, hosts your own servers,
and ships Microsoft / Xbox Live sign-in and offline play as equal first-class
options.

The principles that constrain every decision live in
[`docs/PRINCIPLES.md`](docs/PRINCIPLES.md). The release and supply-chain stance
lives in [`docs/SECURITY.md`](docs/SECURITY.md).

## What works today

**Accounts** — sign in with a Microsoft / Xbox Live account (the full PKCE
OAuth → Xbox Live → XSTS → Minecraft Services chain runs in-process), or use
an offline account for LAN and single-player. Multiple accounts coexist in a
switcher.

**Instances** — multiple isolated `.minecraft` directories side-by-side. Each
instance has its own MC version, mod loader, mods, configs, worlds, JVM args,
GPU preference, and custom icon. Switching instance switches Minecraft install
with one click, and several instances can run **concurrently** — the launcher
tracks each running game separately and surfaces them in a running-instances
popover.

**Launching** — per-instance memory and JVM tuning, a GPU preference
(automatic / high-performance / power-saving), and Quick Play: jump straight
into a specific world or server from the launcher on MC 1.20+.

**Mod loaders** — Fabric (Quilt as a Fabric superset), Forge (every era,
1.7.10 through current), NeoForge. Installer logic runs in-process; no
external Forge installer wizard required.

**Add-ons** — search Modrinth + CurseForge inside the launcher for mods,
resource packs, and shaders. Filter by MC version + loader (defaulted to the
active instance), live MC version combobox. Install resolves required
dependencies automatically; optional deps are surfaced as checkboxes in the
dependency dialog. The "Installed" sub-tab manages what is on disk, with
bulk actions and a one-click update check across the whole instance.
Resource packs and shaders can also be installed manually from a file.

**Modpacks** — a browser across four sources — Modrinth, CurseForge, FTB, and
ATLauncher — as a sidebar-level view (not per-instance, since installing a pack
creates a new instance). Browse, or drag-drop import a `.mrpack` or CurseForge
`.zip`. Pack updates carry their version provenance forward and show a
cumulative changelog of what changed; missing-mod and distribution-disabled
cases are surfaced to the user instead of silently failing. Capabilities differ
by source — Modrinth and CurseForge support export and server-side filtering,
FTB and ATLauncher do not.

**Worlds** — per-instance world list with size + recency, zip-backed
backups (Replace / As-copy restore modes), import of an existing world
folder or archive, and a delete-with-confirmation flow.

**Skins and capes** — pick any cape your Microsoft account owns and upload a
skin, with a 3D preview of the result. A built-in skin editor paints directly
on the 3D model or on the flat 2D atlas, with symmetry, poses, an editable
colour palette, and full undo history. The editor works without a Microsoft
account, so an offline player can author a skin and export it.

**Screenshots** — per-instance gallery of the game's screenshots with
thumbnails, a lightbox viewer, and a simple annotator.

**Logs** — four-source viewer (game log / game console / crash reports /
launcher) with severity colour stripes, search-and-navigate (next/prev +
N-of-M), line-wrap and stack-trace folding toggles, structured crash-report
view with collapsible sections, inline hints on known errors, and one-click
"Share to mclo.gs" with client-side anonymisation of user paths, session
tokens, and LAN IPs. The game-console source captures the process's raw
stdout/stderr, so JVM-level failures that never reach `latest.log` are still
visible.

**Diagnosis and repair** — when a run fails, the launcher reads the logs and
offers the concrete fix rather than just showing the stack trace: raise the
heap, reinstall the loader, disable or reinstall an offending mod, restore
mods a pack is missing. Instances can also be verified against their manifests
and repaired on demand.

**Servers** — a full second mode alongside the client, reached from the
Client / Servers switcher. Run your own Minecraft server from the launcher:
create one on any of seven cores (Vanilla / Fabric / Quilt / Forge / NeoForge /
Paper / Purpur) and switch cores later, start and stop it with a live console,
save and join servers from a servers.dat-backed list, and diagnose start-up
failures. Manage mods or plugins per core (Modrinth + CurseForge for mods,
Modrinth + Hangar for plugins, with update checks), datapacks, the whitelist,
and every vanilla `server.properties` key through a documented editor. Take
scheduled or manual backups. Build a server from an existing instance (or an
instance from a server), then export the assembled server as a `.zip` or upload
it over SFTP to a host you provide.

**Playtime** — per-instance session-time tracking shown on the Overview
tab (total / sessions / last session).

**Updates** — an in-app update check against GitHub Releases with a
one-click self-update: the installer is downloaded, verified against its
`SHA256SUMS` and cosign signature, and launched (Windows NSIS installer;
Linux AppImage in-app, with an honest deb/rpm fallback). No auto-updates
run without you asking.

**System integration** — opt-in "hide launcher to tray when Minecraft
starts" with auto-restore on game exit, a compact mini-window mode,
light/dark/system theme picker, an English / Russian interface language
switch, a relocatable data root, and a guided tour the first time you visit
Manage / Add-ons / Logs / Modpacks / Worlds / Servers / server management.

**Transparency** — every outbound HTTP request goes through a single
chokepoint with a static host allowlist; every process the launcher
spawns goes through one Rust module. Adding a network destination or a
process is a deliberate code change, not an emergent capability.

## Known limitations

- **Windows is the primary target.** Linux (`.AppImage` / `.deb` / `.rpm`)
  and macOS (Universal2 `.dmg`) are **beta** — built, signed, and published
  by CI, but not yet verified end-to-end on those desktops.
- **Not code-signed / notarized.** Windows SmartScreen warns on first run;
  click "More info" → "Run anyway". The macOS `.dmg` is only ad-hoc signed,
  so Gatekeeper quarantines a downloaded copy — clear it with
  `xattr -dr com.apple.quarantine /Applications/Lucerna.app`, or
  right-click → Open on first launch. OS-level signing/notarization is a
  v1.0 concern.

## System requirements

Lucerna itself is a lightweight desktop app — the real hardware demands
come from Minecraft, which the launcher downloads and runs.

To run the launcher:

- Windows 10 (64-bit) or Windows 11 (primary). Beta builds also exist for
  Linux (x86_64) and macOS (Apple Silicon + Intel, via Universal2).
- WebView2 runtime. Preinstalled on Windows 11; on Windows 10 the installer
  downloads and installs it automatically if it is missing.
- An internet connection for the first download of each Minecraft version.
  Once a version is installed, offline play works without one.
- Java is **not** a prerequisite — the launcher downloads the correct Java
  runtime straight from Mojang for each Minecraft version.

To actually play Minecraft (these are Minecraft's own requirements, not the
launcher's):

- 4 GB RAM minimum; 8 GB or more recommended, especially with mods.
- A GPU with modern OpenGL support.
- Disk space scales with use. Each instance is an isolated `.minecraft`
  directory — from a few hundred MB for a vanilla instance to several GB
  for a heavy modpack. Plan for 10+ GB free if you keep multiple instances.

## Install

Download the latest installer from the [Releases page](https://github.com/AntonBabchenko/Lucerna/releases).

- `Lucerna_<version>_x64-setup.exe` — NSIS installer, in English and Russian.

It installs to `%LOCALAPPDATA%\Programs\Lucerna\` by default, per-user, with
no UAC prompt. Launcher data (downloads, accounts, logs) lives at
`%APPDATA%\com.lucerna.app\` by default — the whole data root can be moved
elsewhere from Settings → Storage, which migrates the existing data with it.

Beta builds for other platforms are published to the same Release:

- **Linux** (beta) — `.AppImage` (portable), `.deb`, or `.rpm`.
- **macOS** (beta) — `Lucerna_<version>_universal.dmg` (Universal2, Apple
  Silicon + Intel). The app is ad-hoc signed, not notarized, so clear the
  Gatekeeper quarantine after dragging **Lucerna** to `/Applications`:
  `xattr -dr com.apple.quarantine /Applications/Lucerna.app`.

## Build from source

Prerequisites:

- Rust toolchain (stable). `rustup install stable`.
- Node 24 (CI builds and tests on Node 24) and pnpm 11+. `corepack enable && corepack prepare pnpm@latest --activate`.
- Windows + the Microsoft C++ Build Tools (`Desktop development with C++`).
- A WebView2 runtime (preinstalled on Windows 11).

```powershell
git clone https://github.com/AntonBabchenko/Lucerna.git
cd Lucerna
pnpm install
pnpm tauri dev      # run in development
pnpm tauri build    # produce installer + portable in src-tauri/target/release/bundle/
```

Run the test suite:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
pnpm test
pnpm typecheck
pnpm lint
```

See [`docs/TESTING.md`](docs/TESTING.md) for the full test layout, including
when a single-threaded run is required.

## License

GPL-3.0-or-later. See [`LICENSE`](LICENSE).

If you fork this project you must license your fork under GPL-3.0 or a later
version. This is intentional: it prevents the kind of re-skinned-with-telemetry
fork that motivated Lucerna in the first place.

## Translations

Lucerna ships in English and Russian, and is open for community translation on
[Hosted Weblate](https://hosted.weblate.org/engage/lucerna/) — no coding
required, everything happens in the browser. See
[`docs/TRANSLATING.md`](docs/TRANSLATING.md) to add or improve a language.

## Contributing

Contributions are welcome. The codebase is built in vertical slices with a
spec → plan → TDD implementation → code review → squash-merge cadence.

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for branch, commit, and PR
conventions plus the build and test commands, and [`CLAUDE.md`](CLAUDE.md)
for the working agreement, repo layout, and forbidden patterns.

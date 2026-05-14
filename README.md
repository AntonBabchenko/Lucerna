# FTlauncher

**F**ree **T**auri **Launcher** — a clean, transparent Minecraft launcher for
Windows. No telemetry, no ad injection, no hidden processes, no bundled adware.
The launcher itself is open-source; the Java runtime and Minecraft files come
straight from Mojang.

FTlauncher serves the same use-cases as other launchers — offline / offline play,
easy mods (coming in v0.2.0+), Russian-language UX (coming in v0.2.0+) — but
without any of the hostile behaviour those launchers ship by default.

The principles that constrain every decision live in
[`docs/PRINCIPLES.md`](docs/PRINCIPLES.md). The release and supply-chain stance
lives in [`docs/SECURITY.md`](docs/SECURITY.md).

## v0.1.0 — what works

- Type a name → it persists across launcher restarts (offline account, UUID
  derived from the name).
- Pick a vanilla Minecraft version from the official Mojang manifest
  (releases by default; snapshots / old-alpha / old-beta on demand).
- Click Play → launcher downloads Java + libraries + assets + client.jar,
  then launches Minecraft. Idempotent: re-clicking on the same version
  starts the game in a few seconds.
- Network Activity popover lists every outbound request the launcher made,
  with byte counts and status codes. Allowlist violations are flagged
  in red.
- Logs popover surfaces Minecraft's own logs, crash reports, and the
  launcher's stdout/stderr capture. Search + line numbers + size cap.
- Open mods folder button (no Fabric/Forge support yet — Fabric in v0.2.0,
  Forge in v0.4.0).

## v0.1.0 — known limitations

- Windows-only. macOS / Linux builds in a later release.
- One running Minecraft instance at a time. Multi-instance in v0.3.0.
- No Microsoft (Xbox Live) account support yet — offline accounts only.
  Microsoft auth lands in v0.2.0.
- No mod loader installed by default. The "Open mods folder" button gets
  you to the directory, but vanilla Minecraft does not load mods. Fabric
  support in v0.2.0, Forge in v0.4.0.
- The launcher binary is not code-signed. Windows SmartScreen will warn
  on first run. Click "More info" → "Run anyway". Code signing is a v1.0
  concern.

## Install

Download the latest installer from the [Releases page](https://github.com/AntonBabchenko/FTlauncher/releases).

Two formats are published:

- `FTlauncher_<version>_x64-setup.exe` — NSIS installer (most users).
- `FTlauncher_<version>_x64_en-US.msi` — MSI installer (for IT-managed
  deploys).

Both install to `%LOCALAPPDATA%\Programs\FTlauncher\` by default. Launcher
data (downloads, accounts, logs) lives at
`%APPDATA%\com.ftlauncher.app\`.

## Build from source

Prerequisites:

- Rust toolchain (stable). `rustup install stable`.
- Node 20+ and pnpm 11+. `corepack enable && corepack prepare pnpm@latest --activate`.
- Windows + the Microsoft C++ Build Tools (`Desktop development with C++`).
- A WebView2 runtime (preinstalled on Windows 11).

```powershell
git clone https://github.com/AntonBabchenko/FTlauncher.git
cd FTlauncher
pnpm install
pnpm tauri dev      # run in development
pnpm tauri build    # produce installer + portable in src-tauri/target/release/bundle/
```

Run the test suite:

```powershell
cd src-tauri
cargo test -- --test-threads=1
cd ..
pnpm typecheck
pnpm lint
```

## License

GPL-3.0-or-later. See [`LICENSE`](LICENSE).

If you fork this project you must license your fork under GPL-3.0 or a later
version. This is intentional: it prevents the kind of re-skinned-with-telemetry
fork that motivated FTlauncher in the first place.

## Contributing

The codebase is built in vertical slices. Each slice has a design doc
under `docs/superpowers/specs/` and an implementation plan under
`docs/superpowers/plans/`. The slice cadence is: spec → plan → TDD
implementation → code review → squash-merge.

Read `CLAUDE.md` at the repo root for the working agreement, repo
layout, and forbidden patterns.

# Principles

This document is the Lucerna product and technical constitution. Decisions that conflict with what is written here must update this file first (with rationale in the PR) before code lands.

## Part A — Product values

Lucerna exists to give players a transparent open-source Minecraft launcher: telemetry-free, with no ad injection into the game, no hidden processes, and no bundled adware. The launcher never modifies the Minecraft client jar; the Minecraft we run is the Minecraft Mojang ships. Microsoft / Xbox Live and offline accounts are treated as equal first-class options for legitimate use cases (LAN play, single-player without internet, development testing, players who own Minecraft but prefer not to keep a network session live). Lucerna's aim is to deliver the convenient mod-loader and instance UX players want from a launcher, with none of the telemetry, ad injection, hidden processes, or bundled adware — and to make that verifiable rather than asking for trust. That is the entire mission.

### Hard rules — what we never do

1. **No telemetry, analytics, or fingerprinting.** Not anonymous. Not opt-out. Not "just version stats." If we want to know how many people use the launcher, the GitHub Releases download counter is enough.

2. **No modification of the Minecraft client.** No jar patching. No injection into the main menu. No replaced splash screens. No mods bundled by default. The Minecraft we run is the Minecraft Mojang ships.

3. **No hidden processes.** Every process the launcher spawns is documented in this file (Appendix A) and constructed through the single `process::` module — the one place in the backend a subprocess `Command` is built. A structural test (`src-tauri/tests/structural_no_raw_spawn.rs`) fails the build if a raw `Command` is constructed anywhere else, so the documented list cannot silently fall out of date.

4. **No bundled adware, installer junk, or third-party offers.** The installer ships only the launcher and what it strictly needs to run.

5. **No dark UI patterns.** No "click Cancel to continue." No pre-checked consent boxes. No nag screens. No misleading button labels.

### Positive commitments — what we do

1. **No hidden network calls — enforced, not displayed.** Every outbound HTTP request is funnelled through the single `network::` module, which refuses any host not on the allowlist below *before the request is sent* (`network::allowlist`). A structural test (`src-tauri/tests/structural_no_raw_http.rs`) fails the build if an HTTP client is constructed outside `network::`. The launcher cannot reach a host that is not on this list — that is a property of the code, verifiable by reading it, not a promise.

2. **Allowed network destinations.** This list is compiled into the binary as `network::allowlist::ALLOWED_PATTERNS` and enforced at the chokepoint (Part A positive commitment 1). None are hidden; the table below mirrors the code for human readers.

   | Host pattern | Purpose | Default |
   |---|---|---|
   | `*.minecraft.net`, `*.mojang.com` | Microsoft / Mojang auth, profile, assets | on |
   | `piston-meta.mojang.com`, `piston-data.mojang.com` | Version manifest, libraries | on |
   | `api.github.com/repos/AntonBabchenko/Lucerna/releases` | Launcher self-update check | on |
   | `github.com` | Self-update installer / `SHA256SUMS` / `.cosign.bundle` download (release-asset `browser_download_url`; redirects to a GitHub CDN, which reqwest follows internally — integrity rests on the cosign + SHA-256 verification of the bytes, not on the transport host) | on (only when the user clicks Update) |
   | `api.modrinth.com` | Modrinth mod browser | requested on first open of mod browser |
   | `api.curseforge.com` | CurseForge mod browser | requested on first open of mod browser |
   | `cdn.modrinth.com` | Modrinth mod jar downloads | enabled when user installs from Modrinth |
   | `edge.forgecdn.net` | CurseForge mod jar downloads (primary CDN) | enabled when user installs from CurseForge |
   | `mediafilez.forgecdn.net` | CurseForge mod jar downloads (alternate CDN) | enabled when user installs from CurseForge |
   | `api.modpacks.ch` | FTB modpack browser (metadata) | requested on first open of modpack browser |
   | `dist.modpacks.ch` | FTB modpack file downloads | enabled when user installs an FTB pack |
   | `meta.fabricmc.net`, `maven.fabricmc.net` | Fabric loader meta + libraries | on when user picks Fabric loader |
   | `meta.quiltmc.org`, `maven.quiltmc.org` | Quilt loader meta + libraries | on when user picks Quilt loader |
   | `maven.minecraftforge.net` | Forge installer JARs + library/processor mavens | on when user picks Forge loader |
   | `files.minecraftforge.net` | Forge `promotions_slim.json` (recommended/latest tags) | on when user picks Forge loader |
   | `maven.neoforged.net` | NeoForge installer + library mavens | on when user picks NeoForge loader (v0.4.1) |
   | `login.microsoftonline.com`, `login.live.com`, `user.auth.xboxlive.com`, `xsts.auth.xboxlive.com`, `api.minecraftservices.com` | Microsoft Xbox Live → Minecraft Services authentication chain (cluster C, see `accounts::microsoft::*`) | on when user chooses Microsoft account |

   The Rust constant `network::allowlist::ALLOWED_PATTERNS` is the single source of truth; this table mirrors it for human readers and is kept in sync by code review.

3. **Microsoft and offline accounts are equal first-class citizens.** No UI warnings beyond honest technical disclosures (e.g., "offline accounts cannot connect to online-mode servers"). No "switch to a real license" suggestions. No moralizing copy. The launcher does not judge.

4. **Wire-level release self-audit (planned).** A CI integration test will boot the launcher in a controlled environment with a packet-capture tool, perform only "launch vanilla 1.20.x," and assert that every captured request targets an allowlisted host — an independent, out-of-process confirmation of the in-code enforcement in commitment 1. Status: not yet implemented; tracked in the project roadmap. See `docs/SECURITY.md` Part C.

### Appendix A — documented processes

| Process | Purpose | Spawn site | Lifetime | Stdin / stdout / stderr |
|---|---|---|---|---|
| `javaw.exe` (bundled JRE) | Runs the Minecraft client | `launch::spawn::start` | Until the user closes MC or clicks Stop in the launcher | stdin closed; stdout+stderr → `<instance>/logs/launch-<timestamp>.log` |
| `taskkill.exe` (Windows built-in) | Terminates the running `javaw.exe` and its children when the user clicks Stop | `launch::spawn::stop` | One-shot; exits as soon as the kill request is issued | stdin/stdout/stderr not captured (one-shot system utility) |
| `Lucerna_<ver>_x64-setup.exe` (the official NSIS update installer, downloaded to the app's `updates/` dir and **cosign + SHA-256 verified** before launch) | Installs a newer launcher version when the user clicks Update | `process::spawn_installer` (from `update::install`) | One-shot; the launcher exits immediately after so the installer can replace the locked binary. Windows-only. Never launched for an unverified binary. | inherits the installer's own console (visible NSIS wizard) |
| `explorer.exe` (or OS-default file manager) | Opens a user-clicked folder (currently `<instance>/.minecraft/mods/`) in the OS file manager | `tauri_plugin_opener::OpenerExt::open_path` via the `open_mods_folder` Tauri command | One-shot; the file manager opens (or focuses) a window and the spawn handle exits immediately | stdin/stdout/stderr not captured (GUI process) |
| `java.exe` (bundled JRE, via `-cp`) | Runs SpecialSource bytecode remapper during Forge transitional-era (1.13–1.16) install. Produces byte-identical SRG output that Forge's binarypatcher COPY commands expect. | `process::run_java_processor` (from `forge::patcher::specialsource::run`) | One-shot; exits when remapping is complete. Runs once per Forge install, before play. | stdin closed; stdout+stderr captured and surfaced as install error if exit code ≠ 0 |
| `java.exe` (bundled JRE, via `-cp`) | Runs ForgeAutoRenamingTool (FART) — bytecode remapper for modern-era Forge (1.17+) install. Byte-identical output is required by Forge's binary patches. | `process::run_java_processor` (from `forge::patcher::fart::run`) | One-shot; exits when remapping completes. Runs once per install, before play. | stdin closed; stdout+stderr captured, surfaced as an install error on non-zero exit |
| `java.exe` (bundled JRE, via `-cp`) | Runs AutoRenamingTool (ART) — NeoForge's bytecode remapper for modern-era (1.20.1+) install. | `process::run_java_processor` (from `forge::patcher::art::run`) | One-shot, before play. | stdin closed; stdout+stderr captured, surfaced as an install error on non-zero exit |
| `java.exe` (bundled JRE, via `-cp`) | Runs the binarypatcher ConsoleTool — applies Forge/NeoForge pre-computed binary patches (binarypatcher ≥ 1.2.0). | `process::run_java_processor` (from `forge::patcher::binarypatcher::run_via_java`) | One-shot, before play. | stdin closed; stdout+stderr captured, surfaced as an install error on non-zero exit |
| `java.exe` (bundled JRE, via `-cp`) | Runs the installertools ConsoleTool `PROCESS_MINECRAFT_JAR` task — NeoForge ≥ 21.10's combined remap + binary-patch step. | `process::run_java_processor` (from `forge::patcher::installertools`) | One-shot, before play. | stdin closed; stdout+stderr captured, surfaced as an install error on non-zero exit |

**Selective Java subprocess invocation during install.** Five Forge/NeoForge install processors need byte-fidelity output that Forge's pre-computed binary patches reference by byte offset, so they shell out to the canonical Java implementation rather than a Rust reimplementation: SpecialSource, FART, ART, the binarypatcher ConsoleTool (binarypatcher ≥ 1.2.0 / NeoForge 2.1.2), and the installertools ConsoleTool `PROCESS_MINECRAFT_JAR` task (NeoForge ≥ 21.10). Each is a single processor invocation with bounded args, constructed through `process::run_java_processor`, and listed in the table above.

This does NOT permit running the full Forge `installer.jar` headlessly, or modifying the Minecraft client at runtime. Install steps that do not require byte-fidelity — `jarsplitter` and the non-`PROCESS_MINECRAFT_JAR` installertools tasks — remain pure-Rust.

Whether the binarypatcher and installertools Java steps could be returned to pure Rust is an open question tracked separately in the project roadmap; this table documents the current reality.

The launcher does NOT spawn anything else: no telemetry uploader, no auxiliary watchdog, no helper process. The Java runtime is exactly the one Mojang publishes via the JRE manifest (slice 5), and the Minecraft jar is exactly the one Mojang serves at `piston-data.mojang.com` (slice 4).

## Part B — Technical principles

1. **Stack is fixed:** Rust + Tauri 2.x. **SvelteKit** (Svelte 5 with runes, `adapter-static` in SPA mode) + **TypeScript** + **Tailwind CSS** in the webview. Type-safe IPC via `tauri-specta` (Rust signatures are the single source of truth; TS bindings regenerated on debug build; drift caught at typecheck time).

2. **YAGNI ruthlessly.** No abstractions for hypothetical future requirements. Three similar functions beat a premature generic wrapper. Refactor when the third caller arrives, not before.

3. **Module isolation.** Each module exposes a narrow public API; internals are changeable without touching consumers. Initial module sketch (subject to a later architecture brainstorm):

   - `auth` — Microsoft OAuth + offline account model
   - `versions` — version manifest fetch and parse
   - `instances` — per-instance filesystem isolation, configuration
   - `network` — every outbound HTTP request goes through here (single chokepoint for the allowlist)
   - `launch` — JVM argument construction, process spawn, lifecycle
   - `mods` — Modrinth / CurseForge integration
   - `ui_bridge` — Tauri command surface exposed to the UI

   A file approaching ~500 lines is a signal to split. Not a hard limit; a smell.

4. **Trust internal code, validate at boundaries.** Defensive `try/catch` and `Result` plumbing for impossible cases is forbidden. Validation is mandatory and explicit at: network responses, files on disk, user input via the UI.

5. **Dependencies are deliberate.**
   - A new crate requires PR justification: why needed, what alternatives were considered, dependency tree size impact (`cargo tree | wc -l` before and after).
   - `cargo-deny` is intended to block non-FOSS licenses, known-vulnerable versions, duplicate crates, and unapproved source registries *(planned — not yet wired into CI; see `docs/SECURITY.md` Part A)*.
   - No hard cap on total dependency tree size, but more than 500 transitive crates is a red flag worth pausing for.
   - **npm deps follow the same rule.** A new package requires PR justification (why, alternatives, tree size). `strict-peer-dependencies=true` in `.npmrc`. Build scripts run only when explicitly allowed in `pnpm-workspace.yaml` (`pnpm approve-builds`). Telemetry-shipping packages are rejected outright.

6. **File integrity.**
   - Every byte downloaded onto disk is verified by SHA-1 against an upstream-published checksum.
   - **Exception (v0.2.0):** Fabric and Quilt loader meta endpoints publish profile JSONs that reference loader libraries by `name + url` only — no per-file checksum is exposed. Those libraries are fetched over HTTPS on a trust-on-first-use basis. The exception is opt-in (the user explicitly picks Fabric or Quilt) and scoped to the four hosts in the Part A allowlist table.
   - Vanilla Minecraft libraries, assets, JREs, and the client jar are always SHA-1-verified — no exception there.

7. **Errors are real.**
   - `thiserror` for typed errors in library modules.
   - `anyhow` in application / main code.
   - No `.unwrap()` in production code unless paired with a comment proving the case is unreachable.

8. **Comments explain WHY.** Names explain WHAT. Comments explain non-obvious constraints, invariants, or workarounds. No `// added for issue #123`. No `// removed feature X` markers — deleted code is gone; commit history records why.

9. **Testing.**
   - Unit tests mandatory for: manifest parsers, version JSON parsing, launch argument construction, authentication flows.
   - Integration tests for the full pipeline: select version → download → launch → exit cleanly.
   - UI tests are pragmatic, not mandatory. Test what a user can name as a behavior, not pixel positions.

# Principles

This document is the FTlauncher product and technical constitution. Decisions that conflict with what is written here must update this file first (with rationale in the PR) before code lands.

## Part A — Product values

The project exists because other launchers serves a real use-case (offline play, easy mods, Russian-language UX) but the way other launchers delivers that use-case is hostile to users: telemetry, hidden processes, ad injection into the Minecraft client, bundled adware. FTlauncher delivers the same use-case without any of that.

### Hard rules — what we never do

1. **No telemetry, analytics, or fingerprinting.** Not anonymous. Not opt-out. Not "just version stats." If we want to know how many people use the launcher, the GitHub Releases download counter is enough.

2. **No modification of the Minecraft client.** No jar patching. No injection into the main menu. No replaced splash screens. No mods bundled by default. The Minecraft we run is the Minecraft Mojang ships.

3. **No hidden processes.** Every process the launcher spawns is documented in this file (Appendix A) and visible to the user in a "Processes" panel in the launcher UI.

4. **No bundled adware, installer junk, or third-party offers.** The installer ships only the launcher and what it strictly needs to run.

5. **No dark UI patterns.** No "click Cancel to continue." No pre-checked consent boxes. No nag screens. No misleading button labels.

### Positive commitments — what we do

1. **Network activity is visible.** The launcher has a "Network Activity" panel that lists every outbound request (URL, byte count, what initiated it). Nothing is invisible.

2. **Default-allowed network destinations** (all toggleable in Settings; none hidden):

   | Host pattern | Purpose | Default |
   |---|---|---|
   | `*.minecraft.net`, `*.mojang.com` | Microsoft / Mojang auth, profile, assets | on |
   | `piston-meta.mojang.com`, `piston-data.mojang.com` | Version manifest, libraries | on |
   | `api.github.com/repos/AntonBabchenko/FTlauncher/releases` | Launcher self-update check | on, off-able |
   | `api.modrinth.com` | Modrinth mod browser | requested on first open of mod browser |
   | `api.curseforge.com` | CurseForge mod browser | requested on first open of mod browser |
   | `meta.fabricmc.net`, `maven.fabricmc.net` | Fabric loader meta + libraries | on when user picks Fabric loader |
   | `meta.quiltmc.org`, `maven.quiltmc.org` | Quilt loader meta + libraries | on when user picks Quilt loader |

   The list in code (`src-tauri/src/network/allowlist.rs`) is the single source of truth. A CI test asserts this table matches that file.

3. **Microsoft and offline accounts are equal first-class citizens.** No UI warnings beyond honest technical disclosures (e.g., "offline accounts cannot connect to online-mode servers"). No "switch to a real license" suggestions. No moralizing copy. The launcher does not judge.

4. **Self-audit on every release.** An integration test boots the launcher in a sandbox with a network sniffer, performs only "launch vanilla 1.20.x," and asserts that no outbound request hits any host outside the table above. Failing this test fails the release.

### Appendix A — documented processes

| Process | Purpose | Spawn site | Lifetime | Stdin / stdout / stderr |
|---|---|---|---|---|
| `javaw.exe` (bundled JRE) | Runs the Minecraft client | `launch::spawn::start` | Until the user closes MC or clicks Stop in the launcher | stdin closed; stdout+stderr → `<instance>/logs/launch-<timestamp>.log` |
| `taskkill.exe` (Windows built-in) | Terminates the running `javaw.exe` and its children when the user clicks Stop | `launch::spawn::stop` | One-shot; exits as soon as the kill request is issued | stdin/stdout/stderr not captured (one-shot system utility) |
| `explorer.exe` (or OS-default file manager) | Opens a user-clicked folder (currently `<instance>/.minecraft/mods/`) in the OS file manager | `tauri_plugin_opener::OpenerExt::open_path` via the `open_mods_folder` Tauri command | One-shot; the file manager opens (or focuses) a window and the spawn handle exits immediately | stdin/stdout/stderr not captured (GUI process) |

The launcher does NOT spawn anything else: no telemetry uploader, no auxiliary watchdog, no helper process. The Java runtime is exactly the one Mojang publishes via the JRE manifest (slice 5), and the Minecraft jar is exactly the one Mojang serves at `piston-data.mojang.com` (slice 4).

## Part B — Technical principles

1. **Stack is fixed:** Rust + Tauri 2.x. **SvelteKit** (Svelte 5 with runes, `adapter-static` in SPA mode) + **TypeScript** + **Tailwind CSS** in the webview. Type-safe IPC via `tauri-specta` (Rust signatures are the single source of truth; TS bindings regenerated on debug build; drift caught at typecheck time). See `docs/superpowers/specs/2026-05-12-ui-framework-design.md` for the rationale.

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
   - `cargo-deny` blocks non-FOSS licenses, known-vulnerable versions, duplicate crates, unapproved source registries.
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

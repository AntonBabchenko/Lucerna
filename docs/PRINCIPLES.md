# Principles

This document is the Lucerna product and technical constitution. Decisions that conflict with what is written here must update this file first (with rationale in the PR) before code lands.

## Part A — Product values

Lucerna exists to give players a transparent open-source Minecraft launcher: telemetry-free, with no ad injection into the game, no hidden processes, and no bundled adware. The launcher never modifies the Minecraft client jar; the Minecraft we run is the Minecraft Mojang ships. Microsoft / Xbox Live and offline accounts are treated as equal first-class options for legitimate use cases (LAN play, single-player without internet, development testing, players who own Minecraft but prefer not to keep a network session live). Lucerna's aim is to deliver the convenient mod-loader and instance UX players want from a launcher, with none of the telemetry, ad injection, hidden processes, or bundled adware — and to make that verifiable rather than asking for trust. That is the entire mission.

### Hard rules — what we never do

1. **No telemetry, analytics, or fingerprinting.** Not anonymous. Not opt-out. Not "just version stats." If we want to know how many people use the launcher, the GitHub Releases download counter is enough.

2. **No modification of the Minecraft client.** No jar patching. No injection into the main menu. No replaced splash screens. No mods bundled by default. The Minecraft we run is the Minecraft Mojang ships.

3. **No hidden processes.** Every process the launcher spawns is documented in this file (Appendix A) and constructed through the single `process::` module — the one place in the backend a subprocess `Command` is built. A structural test (`src-tauri/tests/structural_no_raw_spawn.rs`) fails the build if a raw `Command` is constructed anywhere else, so no spawn can be added outside the chokepoint. Note what that guard does and does not buy: it constrains *where* a process may be spawned, but it cannot read this table, so keeping Appendix A complete is a review responsibility. Adding a spawn site without adding its row is a reviewable defect, not something CI will catch.

4. **No bundled adware, installer junk, or third-party offers.** The installer ships only the launcher and what it strictly needs to run.

5. **No dark UI patterns.** No "click Cancel to continue." No pre-checked consent boxes. No nag screens. No misleading button labels.

### Positive commitments — what we do

1. **No hidden network calls — enforced, not displayed.** Every outbound HTTP request is funnelled through the single `network::` module, which refuses any host not on the allowlist below *before the request is sent* (`network::allowlist`). A structural test (`src-tauri/tests/structural_no_raw_http.rs`) fails the build if an HTTP client is constructed outside `network::`. The launcher cannot reach a host that is not on this list — that is a property of the code, verifiable by reading it, not a promise. One deliberate escape hatch qualifies that absolute: the `LUCERNA_EXTRA_ALLOWED_HOSTS` environment variable adds patterns at runtime, so integration tests can point the launcher at a local mock server. It is empty unless an operator sets it, and it is documented as an accepted trade-off in [`SECURITY.md`](SECURITY.md) Part C.

2. **Allowed network destinations.** This list is compiled into the binary as `network::allowlist::ALLOWED_PATTERNS` and enforced at the chokepoint (Part A positive commitment 1). None are hidden; the table below mirrors the code for human readers.

   | Host pattern | Purpose | Default |
   |---|---|---|
   | `*.minecraft.net`, `*.mojang.com` | Microsoft / Mojang auth, profile, assets | on |
   | `piston-meta.mojang.com`, `piston-data.mojang.com` | Version manifest, libraries | on |
   | `api.github.com` | Launcher self-update check (release lookup for this repo; the allowlist matches on host, not path) | on |
   | `github.com` | Self-update installer / `SHA256SUMS` / `.cosign.bundle` download (release-asset `browser_download_url`; redirects to a GitHub CDN, which reqwest follows internally — integrity rests on the cosign + SHA-256 verification of the bytes, not on the transport host) | on (only when the user clicks Update) |
   | `api.modrinth.com` | Modrinth mod browser | requested on first open of mod browser |
   | `api.curseforge.com` | CurseForge mod browser | requested on first open of mod browser |
   | `cdn.modrinth.com` | Modrinth mod jar downloads | enabled when user installs from Modrinth |
   | `edge.forgecdn.net` | CurseForge mod jar downloads (primary CDN) | enabled when user installs from CurseForge |
   | `mediafilez.forgecdn.net` | CurseForge mod jar downloads (alternate CDN) | enabled when user installs from CurseForge |
   | `api.mclo.gs`, `mclo.gs` | Log-share upload to the mclo.gs paste service (log is anonymised client-side before upload and shown to the user first) | only when the user clicks Share in the Logs viewer |
   | `api.modpacks.ch` | FTB modpack browser (metadata) | requested on first open of modpack browser |
   | `dist.modpacks.ch` | FTB modpack file downloads | enabled when user installs an FTB pack |
   | `api.atlauncher.com` | ATLauncher modpack catalogue (metadata) | requested on first open of ATLauncher modpack browser |
   | `download.nodecdn.net` | ATLauncher pack manifest (`Configs.json`) + mod file downloads | enabled when user installs an ATLauncher pack |
   | `vanillatweaks.net` | Vanilla Tweaks datapack builder — per-MC-version category listing, and the zip the site builds from the selected packs (one host serves both) | requested when the user opens the Vanilla Tweaks builder or checks those packs for updates |
   | `meta.fabricmc.net`, `maven.fabricmc.net` | Fabric loader meta + libraries | on when user picks Fabric loader |
   | `meta.quiltmc.org`, `maven.quiltmc.org` | Quilt loader meta + libraries | on when user picks Quilt loader |
   | `maven.minecraftforge.net` | Forge installer JARs + library/processor mavens | on when user picks Forge loader |
   | `files.minecraftforge.net` | Forge `promotions_slim.json` (recommended/latest tags) | on when user picks Forge loader |
   | `maven.neoforged.net` | NeoForge installer + library mavens | on when user picks NeoForge loader (v0.4.1) |
   | `login.microsoftonline.com`, `login.live.com`, `user.auth.xboxlive.com`, `xsts.auth.xboxlive.com`, `api.minecraftservices.com` | Microsoft Xbox Live → Minecraft Services authentication chain (cluster C, see `accounts::microsoft::*`) | on when user chooses Microsoft account |
   | `api.ipify.org` | Public-IP echo for own-server port-forward guidance (returns only the caller's public IP as plain text; no cookies, no request body) | user-initiated, on-demand only — when the user opens the own-server hosting view and asks for their public address (never automatic; no UPnP) |
   | `fill.papermc.io`, `fill-data.papermc.io` | Paper server core builds (Fill v3 API + jar CDN) | on when user picks the Paper core |
   | `api.purpurmc.org` | Purpur server core builds | on when user picks the Purpur core |
   | `hangar.papermc.io`, `hangarcdn.papermc.io` | Hangar plugin browser + Hangar-hosted plugin files | requested on first open of the plugin browser |
   | `api.anthropic.com`, `generativelanguage.googleapis.com`, `api.groq.com` | AI translation pre-fill (opt-in, user's own API key) — the chat-completion endpoint of whichever provider the user selects; only that one host is contacted. The fourth provider, `Local`, is not a host on this list at all — see commitment 5 | off by default; only when the user turns on AI translation and starts a pre-fill run |

   The Rust constant `network::allowlist::ALLOWED_PATTERNS` is the single source of truth; this table mirrors it for human readers and is kept in sync by code review.

3. **Second sanctioned outbound channel — user-initiated SFTP (own-server upload).** The HTTP allowlist in commitment 2 covers all launcher-chosen endpoints. A second, narrower outbound channel exists: **SFTP upload** to a **user-provided** host, used exclusively by the "own server" feature (slice 3) to transfer the user's assembled server archive to their own machine.

   This channel is intentionally outside the HTTP allowlist because the destination is the user's own server — explicitly configured by the user — not a launcher-chosen endpoint or any form of telemetry. It is bounded as follows:

   - **Module isolation:** all `russh`/`russh-sftp` client construction is confined to `src-tauri/src/servers_runtime/transfer.rs`. A structural guard (`src-tauri/tests/structural_no_raw_sftp.rs`) fails the build if SSH/SFTP client code is instantiated anywhere else, keeping the outbound surface enumerable and the spirit of the single-chokepoint principle intact.
   - **Credential handling:** the SFTP password is stored in the OS keyring (via the same keychain abstraction used for Microsoft tokens) — never written to `server.json`, config files, or logs.
   - **Host identity:** connection uses trust-on-first-use (TOFU): the server's SHA-256 fingerprint is stored on first connect; a changed fingerprint blocks the upload and prompts the user to re-confirm. RSA host keys are excluded (see `docs/SECURITY.md` Part F).
   - **Trigger:** SFTP only runs when the user explicitly initiates an upload. It is never called during normal launcher operation (browsing mods, launching instances, etc.).

4. **Third sanctioned outbound channel — user-consented server ping (opt-in, default off).** Showing a saved server's player count needs a Server List Ping to a host **the user typed**. Such a host can never join the commitment-2 allowlist — that would weaken the allowlist for downloads too. Instead it goes through a separate, narrower tier gated on a standing user permission:

   - **Off by default.** `GeneralSettings.allow_server_ping` starts false. While it is false the launcher sends no packet to any user-supplied host; the servers list says the status feature is off and points at the setting.
   - **Unbypassable by construction.** All dialing lives in `src-tauri/src/network/consent.rs`, behind an opaque `ConsentedTcp` whose socket is a private field — so the only way to obtain one is a constructor that re-reads the permission first. Consent is never cached: the settings file is read on every dial, so turning the permission off takes effect immediately.
   - **Structural guard.** `src-tauri/tests/structural_consented_dial.rs` fails the build if `TcpStream` appears outside that one file, if `UdpSocket` appears anywhere, or if the consent check is removed from `ConsentedTcp::open`.
   - **Bounded.** At most 4 simultaneous dials process-wide, a 3 s connect timeout, a 5 s exchange timeout and a 256 KiB response cap — a status check, not a scanner.
   - **Trigger.** Only while a saved-server list is open on screen, or on an explicit refresh. Closing the list stops the sweep — no dial is started after it closes. Never on a timer, never in the background.
   - **Scope.** Only addresses already in that instance's `servers.dat` are dialed. Note that a modpack's `overrides/` can legitimately ship its own `servers.dat` entries (many packs do, to point at their own server), so the list is not necessarily hand-typed — the permission covers *the list*, and the UI copy says so rather than implying otherwise.
   - **Data.** Player counts, the version string and the MOTD are read; the `players.sample` list of other players' names is deliberately not. Nothing is persisted, exported, or sent anywhere else, and server addresses are kept out of the launcher log.
   - **Disclosure.** The setting states plainly that the server owner sees the user's IP address — the same exposure as joining that server.

5. **Fourth sanctioned outbound channel — the loopback seam (a user's own model server).** Choosing `Local` as the AI translation provider means posting to `127.0.0.1` on a user-supplied port. That is neither a launcher-chosen internet destination nor a user-typed remote host, so neither commitment 2 nor commitment 4 fits it — and adding `127.0.0.1` to `ALLOWED_PATTERNS` would let *every* code path in the launcher reach *every* local port. It gets its own narrow seam instead:

   - **Host is not caller-supplied.** `src-tauri/src/network/loopback.rs` holds `127.0.0.1` as a compile-time constant; the caller chooses only the port and the path.
   - **Structural guard.** `src-tauri/tests/structural_loopback_confined.rs` fails the build if anything outside `l10n::prefill` calls the seam, so it cannot grow into a general "reach any local port" capability.
   - **Consent is a type, not an ordering rule.** Every function in `l10n::prefill::provider` that reaches a model requires a `network::consent::AiConsent` — a token whose field is private to `network::consent`, so the only way to hold one is to have passed the permission check.
   - **Nothing leaves the machine.** This is the one AI option that sends no data anywhere: the strings go to a server the user is running themselves.

6. **Microsoft and offline accounts are equal first-class citizens.** No UI warnings beyond honest technical disclosures (e.g., "offline accounts cannot connect to online-mode servers"). No "switch to a real license" suggestions. No moralizing copy. The launcher does not judge.

7. **Wire-level release self-audit (planned).** A CI integration test will boot the launcher in a controlled environment with a packet-capture tool, perform only "launch vanilla 1.20.x," and assert that every captured request targets an allowlisted host — an independent, out-of-process confirmation of the in-code enforcement in commitment 1. Status: not yet implemented; tracked in the project roadmap. See `docs/SECURITY.md` Part C.

### Appendix A — documented processes

| Process | Purpose | Spawn site | Lifetime | Stdin / stdout / stderr |
|---|---|---|---|---|
| `javaw.exe` (bundled JRE) | Runs the Minecraft client. **One per running instance** — several instances can run at once, tracked in a process registry keyed by instance id | `process::spawn_minecraft` (via `launch::spawn::start`) | Until the user closes that instance's MC or clicks Stop for it | stdin closed; stdout+stderr → `<instance>/logs/launch-<timestamp>.log` |
| `taskkill.exe` (Windows built-in) | Terminates one instance's `javaw.exe` and its children when the user clicks Stop | `process::taskkill_tree` (via `platform::kill_process_tree`, from `launch::spawn::stop`) | One-shot; exits as soon as the kill request is issued | stdin/stdout/stderr not captured (one-shot system utility) |
| `Lucerna_<ver>_x64-setup.exe` (the official NSIS update installer, downloaded to the app's `updates/` dir and **cosign + SHA-256 verified** before launch) | Installs a newer launcher version when the user clicks Update | `process::spawn_installer` (from `update::install`) | One-shot; the launcher exits immediately after so the installer can replace the locked binary. Windows-only. Never launched for an unverified binary. | inherits the installer's own console (visible NSIS wizard) |
| `explorer.exe` (or OS-default file manager) | Opens a user-clicked folder (currently `<instance>/.minecraft/mods/`) in the OS file manager | `tauri_plugin_opener::OpenerExt::open_path` via the `open_mods_folder` Tauri command | One-shot; the file manager opens (or focuses) a window and the spawn handle exits immediately | stdin/stdout/stderr not captured (GUI process) |
| `explorer.exe` (or OS-default file manager) | Opens the screenshots folder, or reveals a specific screenshot, in the OS file manager (screenshot viewer) | `tauri_plugin_opener::OpenerExt::open_path` / `reveal_item_in_dir` via the `open_screenshots_folder` / `reveal_screenshot` Tauri commands | One-shot; the file manager opens (or focuses) a window and the spawn handle exits immediately | stdin/stdout/stderr not captured (GUI process) |
| `explorer.exe` (or OS-default file manager) | Reveals a just-created desktop shortcut, from the "Open folder" action on the shortcut-created toast | `@tauri-apps/plugin-opener`'s `revealItemInDir` (frontend, `CreateShortcutDialog`) | One-shot; the file manager opens (or focuses) a window and the spawn handle exits immediately | stdin/stdout/stderr not captured (GUI process) |
| `java.exe` (bundled JRE, via `-cp`) | Runs SpecialSource bytecode remapper during Forge transitional-era (1.13–1.16) install. Produces byte-identical SRG output that Forge's binarypatcher COPY commands expect. | `process::run_java_processor` (from `forge::patcher::specialsource::run`) | One-shot; exits when remapping is complete. Runs once per Forge install, before play. | stdin closed; stdout+stderr captured and surfaced as install error if exit code ≠ 0 |
| `java.exe` (bundled JRE, via `-cp`) | Runs ForgeAutoRenamingTool (FART) — bytecode remapper for modern-era Forge (1.17+) install. Byte-identical output is required by Forge's binary patches. | `process::run_java_processor` (from `forge::patcher::fart::run`) | One-shot; exits when remapping completes. Runs once per install, before play. | stdin closed; stdout+stderr captured, surfaced as an install error on non-zero exit |
| `java.exe` (bundled JRE, via `-cp`) | Runs AutoRenamingTool (ART) — NeoForge's bytecode remapper for modern-era (1.20.1+) install. | `process::run_java_processor` (from `forge::patcher::art::run`) | One-shot, before play. | stdin closed; stdout+stderr captured, surfaced as an install error on non-zero exit |
| `java.exe` (bundled JRE, via `-cp`) | Runs the binarypatcher ConsoleTool — applies Forge/NeoForge pre-computed binary patches (binarypatcher ≥ 1.2.0). | `process::run_java_processor` (from `forge::patcher::binarypatcher::run_via_java`) | One-shot, before play. | stdin closed; stdout+stderr captured, surfaced as an install error on non-zero exit |
| `java.exe` (bundled JRE, via `-cp`) | Runs the installertools ConsoleTool `PROCESS_MINECRAFT_JAR` task — NeoForge ≥ 21.10's combined remap + binary-patch step. | `process::run_java_processor` (from `forge::patcher::installertools`) | One-shot, before play. | stdin closed; stdout+stderr captured, surfaced as an install error on non-zero exit |
| `java.exe` (bundled JRE) | Runs a **Minecraft server** the user created in Servers mode | `process::spawn_server` (from `servers_runtime::runtime`) | Until the user stops the server or it exits | stdin **piped** — this is the console command channel, unlike the client, whose stdin is closed; stdout+stderr piped into the live console and the server log. `CREATE_NO_WINDOW` on Windows so the console `java.exe` does not flash a window |
| `java.exe` (bundled JRE) | Runs a server loader's `installServer` step when creating a Fabric / Quilt / Forge / NeoForge server | `process::install_server` (from `servers_runtime::create`) | One-shot; exits when the server install completes | stdout+stderr captured and surfaced as a create error on non-zero exit |
| `ipconfig` (Windows built-in) | Reads the machine's local IPv4 addresses to show the LAN address for a server you host | `process::local_ipv4_addresses` | One-shot | stdout captured and parsed; nothing is sent anywhere |
| `netsh` (Windows built-in) | Checks whether the launcher's inbound firewall rule for a hosted server already exists | `process::firewall_rule_present` | One-shot | stdout captured and matched against the rule name |
| `powershell` → `netsh` (Windows built-ins) | Adds or removes the inbound firewall rule for a server you host. Runs **elevated** (`Start-Process -Verb RunAs`) because creating a firewall rule requires administrator rights, and with `-WindowStyle Hidden` so the elevated helper does not flash a console. Windows shows its own UAC consent prompt — the launcher cannot suppress it, and nothing happens if you decline. | `process::firewall_add_rule_elevated` / `process::firewall_remove_rule_elevated` | One-shot | not captured (separate elevated process) |
| `sh` (Linux) | Waits for the launcher's own PID to exit, then `exec`s the newly downloaded AppImage — the self-update handoff on Linux | `process::spawn_appimage_relaunch` | Detached; outlives the launcher process by design, exits once the new AppImage takes over | detached; not captured |

**Selective Java subprocess invocation during install.** Five Forge/NeoForge install processors need byte-fidelity output that Forge's pre-computed binary patches reference by byte offset, so they shell out to the canonical Java implementation rather than a Rust reimplementation: SpecialSource, FART, ART, the binarypatcher ConsoleTool (binarypatcher ≥ 1.2.0 / NeoForge 2.1.2), and the installertools ConsoleTool `PROCESS_MINECRAFT_JAR` task (NeoForge ≥ 21.10). Each is a single processor invocation with bounded args, constructed through `process::run_java_processor`, and listed in the table above.

This does NOT permit running the full Forge `installer.jar` headlessly, or modifying the Minecraft client at runtime. Install steps that do not require byte-fidelity — `jarsplitter` and the non-`PROCESS_MINECRAFT_JAR` installertools tasks — remain pure-Rust.

Whether the binarypatcher and installertools Java steps could be returned to pure Rust is an open question tracked separately in the project roadmap; this table documents the current reality.

The launcher does NOT spawn anything else: no telemetry uploader, no auxiliary watchdog, no helper process. The Java runtime is exactly the one Mojang publishes via the JRE manifest (slice 5), and the Minecraft jar is exactly the one Mojang serves at `piston-data.mojang.com` (slice 4).

### Appendix B — structural guards

The commitments above are not honour-system rules; most are enforced by tests in `src-tauri/tests/` that fail the build when bypassed. The full set:

| Guard | Enforces |
|---|---|
| `structural_no_raw_http.rs` | No HTTP client construction outside `network::` (Part A commitment 1) |
| `structural_no_raw_spawn.rs` | No subprocess `Command` outside `process::`, plus an allowlist for `tauri_plugin_opener` call sites (Hard rule 3) |
| `structural_no_raw_sftp.rs` | No SFTP session construction outside `servers_runtime::transfer` (Part A commitment 3) |
| `structural_consented_dial.rs` | No TCP dialing outside `network::consent`, no raw UDP anywhere, and the consent check still present in `ConsentedTcp::open` (Part A commitment 4) |
| `structural_loopback_confined.rs` | No call into the `127.0.0.1` seam (`network::loopback`) outside `l10n::prefill` (Part A commitment 5) |
| `structural_no_sync_reconcile.rs` | A `#[tauri::command]` that runs the mods/plugins reconcile scan must be `async` — a synchronous command runs on the main thread and freezes the window for the length of the scan |
| `structural_no_inplace_mods_write.rs` | No raw file write under `src/mods/`, `src/datapacks/`, or `src/worlds/` outside `mods::store` (instance side) and `mods::cache` (store side). Instance mod jars and world datapack links are hardlinks to one shared physical file, so an in-place write — `fs::copy` included, since it opens the destination with truncate — corrupts every instance or world sharing it. Only write-to-temp-then-rename is safe. |
| `structural_no_blind_err_swallow.rs` | No `Err` arm with an empty body (unless it carries a match guard naming the error, which is the discrimination this asks for), and no discarded `Result` from an fs `rename`/`write` without a justification comment. Removals are excluded — see the module doc. Enforces the mechanically-checkable part of principle B.7's last two bullets; a swallowed *removal* on a recovery path and a promised-but-missing log remain review's job. |
| `structural_platform_chokepoint.rs` | OS-specific behaviour stays behind the `platform::` seam rather than leaking `#[cfg(windows)]` across the codebase |
| `structural_no_env_mutation.rs` | No `std::env::set_var` in production code — env overrides go through `test_seam` (this is what removed the need for single-threaded test runs) |
| `structural_installer_branding.rs` | The NSIS installer keeps Lucerna branding assets wired up |
| `structural_installmode_currentuser.rs` | The installer stays per-user (`currentUser`), so updating never needs administrator rights |

These guards constrain *where* code may do a thing. They cannot verify that this document's tables are complete — that remains a code-review responsibility.

## Part B — Technical principles

1. **Stack is fixed:** Rust + Tauri 2.x. **SvelteKit** (Svelte 5 with runes, `adapter-static` in SPA mode) + **TypeScript** + **Tailwind CSS** in the webview. Type-safe IPC via `tauri-specta` (Rust signatures are the single source of truth; TS bindings regenerated on debug build; drift caught at typecheck time).

2. **YAGNI ruthlessly.** No abstractions for hypothetical future requirements. Three similar functions beat a premature generic wrapper. Refactor when the third caller arrives, not before.

3. **Module isolation.** Each module exposes a narrow public API; internals are changeable without touching consumers. Initial module sketch (subject to a later architecture brainstorm):

   - `accounts` — Microsoft OAuth + offline account model
   - `versions` — version manifest fetch and parse
   - `instances` — per-instance filesystem isolation, configuration
   - `network` — every outbound HTTP request goes through here (single chokepoint for the allowlist)
   - `launch` — JVM argument construction, process spawn, lifecycle
   - `mods` — Modrinth / CurseForge integration
   - `commands` — Tauri command surface exposed to the UI

   A file approaching ~500 lines is a signal to split. Not a hard limit; a smell.

4. **Trust internal code, validate at boundaries.** Defensive `try/catch` and `Result` plumbing for impossible cases is forbidden. Validation is mandatory and explicit at: network responses, files on disk, user input via the UI.

5. **Dependencies are deliberate.**
   - A new crate requires PR justification: why needed, what alternatives were considered, dependency tree size impact (`cargo tree | wc -l` before and after).
   - `cargo-deny` blocks non-FOSS licenses, known-vulnerable versions, duplicate crates, and unapproved source registries. It is configured in `src-tauri/deny.toml` and enforced by the `cargo-deny` job in `.github/workflows/ci.yml`, which is a required check (see `docs/SECURITY.md` Part A).
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
   - A fallback answers "could not check" restrictively, and never collapses "absent" into "could not tell". (`Path::exists()` collapses them by design — it reports `false` for any stat failure. Use `try_exists()`, or claim the name atomically.)
   - A rename or write on a recovery path — rollback, cleanup, or any step that runs because something already failed — is never discarded silently.

8. **Comments explain WHY.** Names explain WHAT. Comments explain non-obvious constraints, invariants, or workarounds. No `// added for issue #123`. No `// removed feature X` markers — deleted code is gone; commit history records why.

9. **Testing.**
   - Unit tests mandatory for: manifest parsers, version JSON parsing, launch argument construction, authentication flows.
   - Integration tests for the full pipeline: select version → download → launch → exit cleanly.
   - UI tests are pragmatic, not mandatory. Test what a user can name as a behavior, not pixel positions.

# Lucerna — Working Agreement

This file is the working agreement between the human maintainer and Claude. Claude loads it automatically every session. Keep it short, accurate, and actionable.

> Sister file: [`AGENTS.md`](AGENTS.md) (loaded by Codex). Keep both in sync.

## What this project is

Lucerna is a clean, transparent open-source Minecraft Java Edition launcher for Windows, with beta builds for Linux and macOS. It supports Microsoft / Xbox Live sign-in and offline play (LAN, single-player without internet, development testing) as equal first-class options, integrates the official Modrinth and CurseForge mod APIs, and isolates per-instance Minecraft state. It ships no telemetry, no ad injection, no hidden processes, and no bundled adware, and it never modifies the Minecraft client. (The Microsoft sign-in chain works end-to-end: Microsoft approved Lucerna's Azure app, so live sign-in is functional. A typed pending-approval response is retained as a defensive fallback should the registration ever lapse; offline play is a first-class option regardless.) The UI ships in English and Russian, switchable live in-app (Settings → Appearance).

The principles that constrain every decision live in [`docs/PRINCIPLES.md`](docs/PRINCIPLES.md). The release and supply chain stance lives in [`docs/SECURITY.md`](docs/SECURITY.md). Read both before significant changes. The UI design system — tokens, component vocabulary, and which variant/element to use when — is documented in [`docs/DESIGN.md`](docs/DESIGN.md); consult it before UI changes.

## Core principle — do it properly

The goal of this project is to do things properly, not to ship quick patches. When a symptom can be silenced with a small change but the underlying design is wrong, fix the cause — and say so. Follow the established conventions of the domain (web UI pagination / filtering / sorting, Rust idioms, the feature lifecycle below); when a proposed change fights a convention, surface that *before* writing code so the trade-off is a deliberate decision. A minimal fix is acceptable only when it is also the correct one; if the proper fix is larger, say that honestly rather than patching silently.

## Feature lifecycle (mandatory, no exceptions)

Every feature follows this sequence:

1. `brainstorming` skill — produces a spec at `docs/superpowers/specs/YYYY-MM-DD-<topic>-design.md`.
2. `writing-plans` skill — produces a plan at `docs/superpowers/plans/YYYY-MM-DD-<topic>-plan.md`.
3. `test-driven-development` skill — required for core logic (authentication, version resolution, manifest verification, launch pipeline). Pragmatic for UI: test behavior the user can name, not pixel polish.
4. `verification-before-completion` skill — run the dev build, exercise the feature, confirm before claiming done.
5. `requesting-code-review` skill (or `code-reviewer` subagent) — before merge. Check every fallback the change introduces or touches against **Fallback discipline** below.
6. Commit and push.

No "small fix" exception. A bugfix is still a feature in this sense — it still gets a spec and a plan, however short.

> The `docs/superpowers/` specs / plans / notes are kept **local-only** (gitignored) — they are the maintainer's working artifacts and are not published in the public repository.

## Git workflow

- **Branches:** short-lived feature branches off `main` (e.g., `feat/auth-microsoft`, `fix/manifest-parser`).
- **Commits:** Conventional Commits prefixes — `feat:`, `fix:`, `chore:`, `docs:`, `test:`, `refactor:`, `build:`, `ci:`.
- **Merges:** squash merge into `main`.
- **Tags:** `vMAJOR.MINOR.PATCH` for releases (semver).

## Repo layout

```
src-tauri/                          Rust backend — launcher core, Tauri shell
src-tauri/src/commands/             Tauri commands exposed to UI (typed via tauri-specta)
src-tauri/src/accounts/microsoft/   Microsoft Xbox Live → Minecraft Services auth chain
src-tauri/src/accounts/keychain.rs  OS keyring wrapper for MS refresh + MC access tokens
src-tauri/src/launch/               Launch pipeline + running-instance registry
src-tauri/src/instances/            Instance model, storage, import
src-tauri/src/servers_runtime/      Own-server lifecycle: cores, plugins, backups, properties
src-tauri/src/verify/               Instance integrity scan + repair
src-tauri/src/data_root/            Relocatable data root (resolve, migrate, fallback)
src-tauri/src/network/              HTTP chokepoint + host allowlist
src-tauri/src/process/              Subprocess chokepoint
src-tauri/tests/                    Rust integration tests (incl. structural guards)
src/                                SvelteKit UI (Svelte 5 runes + TypeScript)
src/routes/                         SvelteKit file-based routes (single-page launcher)
src/lib/ipc/bindings.ts             Generated TS bindings from tauri-specta — do not edit
src/lib/accounts/                   Accounts UI + skins/capes + skin-editor/
src/lib/i18n/                       Locales (en, ru) + generated key union
src/lib/ui/                         Shared design-system primitives
src/app.css                         Tailwind imports + minimal global CSS
src/app.html                        SvelteKit HTML shell (page title lives here)
static/                             Static assets served at root
tools/                              Small CI/dev helper scripts (Node, .mjs)
tests/                              Vitest unit tests
tests-e2e/                          Playwright e2e tests
docs/                               Public docs: PRINCIPLES, SECURITY, DESIGN, TESTING, UI-TESTING (superpowers/ is gitignored, local-only)
.github/workflows/ci.yml            CI: rust-ubuntu/windows/macos, linux-bundle, frontend, e2e, lint, coverage-frontend/rust, cargo-deny, ci-gate (SHA-pinned)
```

## Entry-point commands

- `pnpm install`           install Node deps.
- `pnpm tauri dev`         run launcher in dev mode (Vite + Rust + Tauri webview).
- `pnpm tauri build`       build a release binary.
- `pnpm typecheck`         svelte-kit sync + svelte-check.
- `pnpm test`              run Vitest once.
- `pnpm test:watch`        Vitest watch mode.
- `pnpm test:e2e`          Playwright e2e (functional specs run in CI).
- `pnpm i18n:keys`         regenerate the i18n key union after editing en.json — CI enforces `i18n:keys:check`.
- `pnpm lint`              Biome + Prettier (Svelte) + no-network-calls gate.
- `pnpm lint:no-network`   only the no-network-calls gate.
- `pnpm format`            auto-format with Biome + Prettier (Svelte).
- `cargo test` (in `src-tauri/`)   Rust unit tests.

## Auto-memory location

Persistent project memory lives outside the repository, in the agent's local profile — it is not tracked here.

## Fallback discipline

A fallback is a second path taken when the first is unavailable. Good ones are invisible and safe; bad ones invent a confident answer out of ignorance. Every fallback must answer yes to all four:

1. **Direction** — when the check could not be performed, does it resolve to the restrictive answer?
2. **Discrimination** — does it tell "absent" apart from "could not tell"? (`Path::exists()` does not — it reports `false` for any stat failure.)
3. **Honesty** — does what the user sees describe what actually happened?
4. **Recovery path** — if it runs because something already failed, is its own result checked?

Question 4 is the one that bites: a swallowed error on a rollback or cleanup is where the user's data is in flight. `structural_no_blind_err_swallow.rs` enforces only part of this — a swallowed *removal* on a recovery path, and a promised-but-missing log, are invisible to it. The rest is review's job.

## Forbidden patterns (grep-able quick reference)

These distill the principles from `docs/PRINCIPLES.md` and `docs/SECURITY.md` into stop-words. If a PR would introduce any of these, the answer is no:

- Adding a crate dependency without `cargo-deny` justification in the PR description.
- Outbound network calls from anywhere outside the `network::` module.
- Patching the Minecraft client jar — under any circumstances.
- Writing to `~/.minecraft` directly — only via isolated instance directories.
- `unwrap()` in production code without a comment proving the case is unreachable.
- Any analytics, telemetry, or fingerprinting code path — including "anonymous" or "opt-out" variants.
- Any code path that modifies the Minecraft client at runtime (main menu injection, splash override, default-bundled mods).
- Any HTTP client construction outside the `network::` module (caught by `src-tauri/tests/structural_no_raw_http.rs`).
- Any subprocess spawn outside the `process::` module (caught by `src-tauri/tests/structural_no_raw_spawn.rs`).
- Any write into an instance's or world's content directories outside the `mods::store` chokepoint — instance mod jars and world datapack links are hardlinks to one shared physical file, so an in-place write (including `fs::copy`, which truncates its destination) corrupts every instance or world using it (caught by `src-tauri/tests/structural_no_inplace_mods_write.rs`, which scans `src/mods/`, `src/datapacks/`, and `src/worlds/`).
- An `Err` arm whose block is empty. Add a match guard naming the error you mean (`Err(e) if e.kind() == NotFound => {}` is fine — that is discrimination), or put the reason **inside** the block (caught by `src-tauri/tests/structural_no_blind_err_swallow.rs`).
- Discarding the result of a filesystem `rename` or `write` — via `let _ =` or `.ok()` — without a justification comment. Same rule as `unwrap()`: a discarded rename means the move may not have happened and nobody will know (same guard).

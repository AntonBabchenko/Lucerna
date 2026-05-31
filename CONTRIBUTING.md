# Contributing to Lucerna

Thanks for your interest in Lucerna — a transparent, open-source Minecraft Java
Edition launcher for Windows. This document covers how to build the project and
the conventions for contributing changes.

By participating you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

## Ground rules

Lucerna has a small set of non-negotiable principles — read
[`docs/PRINCIPLES.md`](docs/PRINCIPLES.md) and [`docs/SECURITY.md`](docs/SECURITY.md)
before a substantial change. In short:

- **No telemetry, analytics, or fingerprinting** — in any form, including
  "anonymous" or "opt-out" variants.
- **The Minecraft client is never modified.** We run the Minecraft Mojang ships.
- **All outbound network goes through the `network::` chokepoint** and its static
  host allowlist. Any new host is a deliberate, reviewed code change.
- **All subprocess spawns go through the `process::` module.**
- These last two are enforced by structural tests (`src-tauri/tests/structural_no_raw_http.rs`,
  `structural_no_raw_spawn.rs`) that fail the build if bypassed.

`CLAUDE.md` at the repo root lists the full "forbidden patterns" quick reference.

## Development setup

Prerequisites:

- Rust toolchain (stable) — `rustup install stable`.
- Node 20+ and pnpm 11+ — `corepack enable && corepack prepare pnpm@latest --activate`.
- Windows + the Microsoft C++ Build Tools (`Desktop development with C++`).
- A WebView2 runtime (preinstalled on Windows 11).

```powershell
git clone https://github.com/AntonBabchenko/Lucerna.git
cd Lucerna
pnpm install
pnpm tauri dev      # run the launcher in development
```

## Useful commands

| Command | What it does |
|---|---|
| `pnpm tauri dev` | Run the launcher in dev mode (Vite + Rust + Tauri webview). |
| `pnpm tauri build` | Produce a release binary. |
| `pnpm typecheck` | `svelte-kit sync` + `svelte-check`. |
| `pnpm test` | Run the Vitest unit suite once. |
| `pnpm lint` | Biome + Prettier (Svelte) + the no-network-calls gate. |
| `pnpm format` | Auto-format with Biome + Prettier (Svelte). |
| `cargo test` (in `src-tauri/`) | Rust unit + integration tests. |

Run the full local gate before opening a PR:

```powershell
cd src-tauri
cargo test -- --test-threads=1
cd ..
pnpm typecheck
pnpm lint
pnpm test
```

## Branches and commits

- Work on short-lived feature branches off `main` (e.g. `feat/quick-play`,
  `fix/manifest-parser`).
- Use [Conventional Commits](https://www.conventionalcommits.org/) prefixes:
  `feat:`, `fix:`, `chore:`, `docs:`, `test:`, `refactor:`, `build:`, `ci:`.
- Changes are **squash-merged** into `main`.

## Pull requests

1. Make sure the local gate above is green.
2. Open a PR against `main` and fill in the template.
3. Describe what changed and why; if you added a dependency, justify it (why it's
   needed, alternatives considered, dependency-tree impact) per `docs/PRINCIPLES.md`.
4. A maintainer reviews before merge. CI (Rust tests on Linux + Windows, frontend
   typecheck/test, lint) must pass.

## Reporting bugs and requesting features

Use the GitHub issue templates. For **security vulnerabilities**, do not open a
public issue — use the [GitHub Private Security Advisory](https://github.com/AntonBabchenko/Lucerna/security/advisories/new)
channel described in [`SECURITY.md`](SECURITY.md).

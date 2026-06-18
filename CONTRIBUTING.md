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
- Node 24 (CI builds and tests on Node 24) and pnpm 11+ — `corepack enable && corepack prepare pnpm@11 --activate`.
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

## Microsoft sign-in (Azure app)

Microsoft / Xbox Live sign-in uses an OAuth **public client** (PKCE, **no client
secret** — never add one; a secret cannot be kept secret in a distributed desktop
binary). The official build is tied to an Azure app registration in the
maintainer's Entra tenant via a public `client_id`.

The `client_id` is **not a secret** — it is embedded in every shipped binary and
is public by design. It is, however, an **identity**: any build using it shows
"Lucerna" on the Microsoft consent screen. So:

- **End users:** nothing to do. The official installer has the official
  `client_id` compiled in; just sign in with your own Microsoft account (the one
  that owns Minecraft). No Azure account or env var needed.
- **If you distribute a fork:** register your **own** Azure app and build with
  your own id — do **not** ship under Lucerna's identity:

  ```powershell
  $env:LUCERNA_MS_CLIENT_ID = "<your-azure-app-client-id>"
  pnpm tauri build
  ```

  `LUCERNA_MS_CLIENT_ID` is read at **compile time** ([`oauth.rs`](src-tauri/src/accounts/microsoft/oauth.rs));
  unset, it falls back to the upstream public id, which is fine for local
  development but should not be used for a redistributed build.

To register your own app at [portal.azure.com](https://portal.azure.com)
(Entra ID → App registrations):

- **Supported account types:** personal Microsoft accounts ("consumers").
- **Platform:** Mobile and desktop applications, redirect URI
  `http://127.0.0.1` (loopback — Lucerna binds an ephemeral port at runtime and
  appends `/oauth/callback`).
- **API permissions / scopes:** `XboxLive.signin offline_access`. Xbox/Minecraft
  scopes require Microsoft approval of the app registration before sign-in
  succeeds end-to-end.

## CurseForge API key

CurseForge's API requires an `x-api-key` header on every request, so the
launcher needs a key to browse/download CurseForge mods and modpacks. Unlike
the Microsoft `client_id` (a public identity used with PKCE), this key is an
**application credential** — it cannot be kept truly secret in a distributed
binary, but CurseForge offers no anonymous access, so the least-bad option is
to embed it and be transparent about it (the same approach Prism Launcher
takes). See [`docs/SECURITY.md`](docs/SECURITY.md) Part E.

- **End users:** nothing to do. The official installer has a working key
  compiled in; CurseForge works out of the box. You may optionally set your own
  key in Settings → Integrations — a personal key takes precedence.
- **If you distribute a fork:** register your own key at
  [console.curseforge.com](https://console.curseforge.com) and build with it,
  so you don't share the upstream key's rate limit:

  ```powershell
  $env:LUCERNA_CURSEFORGE_API_KEY = "<your-curseforge-api-key>"
  pnpm tauri build
  ```

  `LUCERNA_CURSEFORGE_API_KEY` is read at **compile time**
  ([`keyring.rs`](src-tauri/src/mods/curseforge/keyring.rs)); unset, the build
  ships no embedded key and falls back to the in-app manual key entry, which is
  fine for local development.

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

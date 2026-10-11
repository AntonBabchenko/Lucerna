# Building Lucerna from source

How to compile and run Lucerna on Windows, Linux and macOS. Windows is the
primary platform; the Linux and macOS builds are beta.

Everything here mirrors what the workflows in [`.github/workflows/`](../.github/workflows/)
install and run: `ci.yml` builds and tests the launcher on Ubuntu, Windows and
macOS, and `release.yml` produces the published installers. If this file and a
workflow ever disagree, the workflow is right and this file has a bug.

## Requirements on every platform

- **Git.**
- **Rust**, installed with [rustup](https://rustup.rs). Lucerna builds with the
  stable toolchain (pinned in `src-tauri/rust-toolchain.toml`), which is what
  rustup installs by default.
- **Node.js 24**, the version CI uses, with **pnpm** enabled through Corepack:

  ```sh
  corepack enable
  ```

  Corepack reads the `packageManager` field of `package.json` and runs exactly
  that pnpm version, so there is no version to pick by hand. Node.js 25 and newer
  no longer include Corepack; on those, run `npm install -g corepack` first. On
  Windows, if `corepack enable` fails with `EPERM`, run it once from a terminal
  opened as administrator.

Then add your platform's native build tools below.

## Windows

- **Microsoft C++ Build Tools** with the **Desktop development with C++**
  workload (a Visual Studio install with the same workload also works). Rustup
  uses the matching MSVC toolchain by default on Windows.
- **WebView2 runtime.** Preinstalled on Windows 11; on Windows 10, install the
  Evergreen runtime from Microsoft if it is missing.

Download links for both are on Tauri's
[prerequisites page](https://v2.tauri.app/start/prerequisites/#windows).

## Linux (beta)

On Debian or Ubuntu, install the packages the CI and release workflows install:

```bash
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  libgtk-3-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libsoup-3.0-dev \
  build-essential \
  libxdo-dev \
  file \
  rpm
```

CI builds on x86_64 Ubuntu only. On another distribution, install the
equivalents from the Linux section of Tauri's
[prerequisites page](https://v2.tauri.app/start/prerequisites/#linux): WebKitGTK
4.1, GTK 3, an AppIndicator library, librsvg, libsoup 3, libxdo and a C
toolchain. You can skip the OpenSSL development package that page lists:
Lucerna's HTTP client uses rustls, not OpenSSL.

## macOS (beta)

Install Xcode's command line tools:

```bash
xcode-select --install
```

A full Xcode install works too; Tauri only requires it for iOS, which Lucerna
does not target. Nothing else is needed: the app uses the system WebKit, and the
macOS CI job installs no extra packages.

## Get the code and run it

```sh
git clone https://github.com/AntonBabchenko/Lucerna.git
cd Lucerna
pnpm install
pnpm tauri dev
```

`pnpm tauri dev` starts the Vite dev server and opens the launcher. Interface
edits reload in place; Rust edits rebuild and restart the app. The first run
compiles the whole Rust dependency tree and takes a while, later runs are
incremental.

## Build release bundles

```sh
pnpm tauri build
```

The bundles for your platform land under `src-tauri/target/release/bundle/`:

| Platform | Output |
|---|---|
| Windows | `nsis/Lucerna_<version>_x64-setup.exe` |
| Linux | `appimage/*.AppImage`, `deb/*.deb`, `rpm/*.rpm` |
| macOS | `macos/Lucerna.app`, `dmg/*.dmg`, for your Mac's own architecture |

To build only the executable, without installers, run
`pnpm tauri build --no-bundle`; it lands in `src-tauri/target/release/`.

On macOS, the release build sets `APPLE_SIGNING_IDENTITY=-` so Tauri ad-hoc signs
the app while bundling. That is not notarization; do the same for a bundle you
mean to run:

```bash
APPLE_SIGNING_IDENTITY=- pnpm tauri build
```

The published `.dmg` is a Universal2 build (Apple Silicon and Intel in one app).
To build that yourself, add both Rust targets and pass the universal target:

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
APPLE_SIGNING_IDENTITY=- pnpm tauri build --target universal-apple-darwin
```

Its bundles land under `src-tauri/target/universal-apple-darwin/release/bundle/`.

## Build-time variables

These are read when the Rust code compiles, so set them in the shell before
`pnpm tauri build` (or `pnpm tauri dev`). All are optional.

| Variable | When unset | When set |
|---|---|---|
| `LUCERNA_CURSEFORGE_API_KEY` | No key is compiled in. CurseForge, and FTB downloads that go through it, ask for your own free key in Settings → Integrations. | The key is compiled into the binary. |
| `LUCERNA_MS_CLIENT_ID` | The upstream public Microsoft client id, fine for a build you use yourself. | Your own Azure app id. Required if you distribute a fork. |
| `LUCERNA_BUILD_TAG`, `LUCERNA_BUILD_COMMIT`, `LUCERNA_BUILD_REPO` | Settings → About says "Local build". | The release identity Settings → About shows and copies. |

Why the CurseForge key and the Microsoft id work this way, and how to register
your own: [Microsoft sign-in](../CONTRIBUTING.md#microsoft-sign-in-azure-app) and
[CurseForge API key](../CONTRIBUTING.md#curseforge-api-key) in CONTRIBUTING.

## Tests

The local gate and the full command list are in
[CONTRIBUTING](../CONTRIBUTING.md#useful-commands); the test layout is in
[`TESTING.md`](TESTING.md).

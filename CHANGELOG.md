# Changelog

All notable changes to Lucerna are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Development happens continuously on `main`. Versions between `0.1.0` and the first
published release were untagged feature milestones; the first packaged public
release is **0.9.0**.

## [Unreleased]

## [0.10.0] — 2026-06-03

### Added
- **Localisation.** The entire UI is available in English and Russian with a
  live in-app language switch (Settings → General) — no restart required.
- **Modpack export.** Round-trip a customised instance back to a `.mrpack` or a
  CurseForge `.zip`, in a lightweight (manifest-only) or full (bundled-files)
  variant.
- **Bulk mod actions.** A compact installed-mods list with multi-select and a
  bulk action bar (enable / disable / delete), an inline nested dependency tree,
  and orphan-dependency safety.
- **Cross-platform foundation (beta).** Linux (x86_64) and macOS (Universal2,
  unsigned / ad-hoc) now build, sign, and publish in CI as beta targets.
  End-to-end Minecraft-launch verification on each desktop is still pending.

### Changed
- **Microsoft / Xbox Live sign-in is now live.** Microsoft approved Lucerna's
  Azure app registration, so the Microsoft sign-in flow completes end-to-end and
  signs you into your Minecraft account. No launcher code change was required —
  the previously pending-approval state simply stopped occurring once Microsoft
  began returning a successful response. Offline accounts remain an equal
  first-class option.
- **Installed-mods tab overhaul.** Denser rows, a pinned "needs attention" bar,
  Updates / Issues quick-filter chips, a single priority status badge per row,
  and unified pagination.
- **Mod-browser filtering.** "Show installed" now defaults to hidden for a
  discovery-first view — the page tops up once the installed set is known, so an
  all-installed first page no longer dead-ends on an empty view — plus a "Match
  this instance" button that restores the loader and Minecraft version to the
  active instance's defaults.
- Clicking **Play** with no account now spotlights the account section instead
  of showing a terse banner.
- The modpack browser now opens as a full-screen modal.
- All native `<select>` dropdowns were replaced with a themeable Select
  component — consistent rendering across platforms and a fix for the dark-mode
  dropdown on Linux / WebKitGTK.

### Fixed
- Transitive mod dependency resolution now installs dependencies-of-dependencies,
  with a per-mod install toast.
- Installing a mod built for the wrong loader is now prevented, and replaying
  onboarding re-arms the contextual tours.
- Native form controls now match the active theme (`color-scheme` set per theme).
- Mod-install failures now explain the cause: "no version for this Minecraft
  version" versus "built for a different loader" (with the latest supported
  version listed per loader), author-disabled distribution names the mod with an
  "Open on &lt;platform&gt;" action, filename conflicts name the already-installed
  file, and SHA-1 checksum failures read in plainer language.
- Long toast messages now wrap instead of being truncated to a single line.
- Lowered the minimum window height so the sidebar's Logs / Settings controls
  sit flush at the bottom instead of leaving empty space below them.
- Orphaned tray icons no longer pile up. With *hide launcher to tray during
  game* enabled, each game launch used to leave a tray icon that never went
  away; the icon is now properly removed when the game closes.

## [0.9.1] — 2026-06-01

### Added
- **Self-update.** On startup the launcher checks GitHub Releases and, when a newer
  version is available, shows a sticky notification with a one-click **Update** button.
  Clicking it downloads the official installer, verifies it (SHA-256 against
  `SHA256SUMS` **and** cosign keyless against the release's `.cosign.bundle`, pinned to
  the release workflow's signing identity), launches it, and exits. An unverified binary
  is never launched. The install is always an explicit click — there is no silent
  background update. A Settings → General toggle ("Check for updates on startup",
  on by default) controls the check, and dismissing the notification suppresses it for
  that version until a newer release appears.

## [0.9.0] — 2026-05-31

### Changed
- **Renamed the project `FTlauncher` → `Lucerna`.** Binary/crate, identifiers
  (`com.lucerna.app`), keyring slots, per-instance directories, user-facing
  strings, and docs were updated. A one-shot, idempotent startup migration moves
  existing launcher data and per-instance directories to the new names.

### Added (feature milestones since 0.1.0)
- **Mod loaders:** Fabric (and Quilt as a Fabric superset), Forge (every era,
  1.7.10 through current), and NeoForge — installer logic runs in-process.
- **Mod browser:** search Modrinth + CurseForge in-app, filter by MC version and
  loader, automatic required-dependency resolution, and a standalone mod-update
  check.
- **Modpacks:** Modrinth and CurseForge modpack browser plus drag-and-drop import
  of `.mrpack` and CurseForge `.zip`, with version provenance carried forward and
  missing / distribution-disabled mods surfaced to the user.
- **Worlds:** per-instance world list with size and last-played, zip-backed
  backups (Replace / As-copy restore), and a guarded delete flow.
- **Logs:** three-source viewer (game / crash reports / launcher) with severity
  colouring, search-and-navigate, line-wrap and stack-trace folding, structured
  crash-report view, and one-click share to mclo.gs with client-side anonymisation.
- **Playtime:** per-instance session-time tracking on the Overview tab.
- **System integration:** opt-in hide-to-tray on launch with auto-restore,
  light/dark/system theme picker, and first-visit guided tours.
- **Microsoft / Xbox Live sign-in** wired up end-to-end (final step gated on
  pending Microsoft Azure app approval; offline accounts remain fully usable).
- **Transparency:** single network chokepoint with a static host allowlist and a
  single subprocess module, both enforced by structural tests.

## [0.1.0] — 2026-05-13

### Added
- First release. Vanilla Minecraft launch, offline accounts, and per-instance
  isolated `.minecraft` directories, with the launcher downloading the correct
  Java runtime per Minecraft version.

[Unreleased]: https://github.com/AntonBabchenko/Lucerna/compare/v0.10.0...main
[0.10.0]: https://github.com/AntonBabchenko/Lucerna/compare/v0.9.1...v0.10.0
[0.9.1]: https://github.com/AntonBabchenko/Lucerna/releases/tag/v0.9.1
[0.9.0]: https://github.com/AntonBabchenko/Lucerna/releases/tag/v0.9.0
[0.1.0]: https://github.com/AntonBabchenko/Lucerna/releases/tag/v0.1.0

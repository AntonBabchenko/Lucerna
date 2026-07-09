# Changelog

All notable changes to Lucerna are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Development happens continuously on `main`. Versions between `0.1.0` and the first
published release were untagged feature milestones; the first packaged public
release is **0.9.0**.

## [Unreleased]

### Fixed

- Tooltips triggered by keyboard focus now appear only on real keyboard focus
  (`:focus-visible`), so they no longer flash when a modal opens or closes.

## [0.17.0] — 2026-07-09

### Added

- **Skin & Cape cosmetics for Microsoft accounts.** From a new "Skin & Cape"
  button under your account, pick from the capes your account owns or hide the
  active one, and upload a new skin (classic or slim) or reset it to the
  default — all through the official Minecraft profile API. A rotatable 3D
  preview shows your skin with the active cape; resetting the skin asks for
  confirmation and offers a one-click restore of the previous one. English and
  Russian.
- **In-app screenshots.** A per-instance Screenshots tab and a global gallery
  reachable from the sidebar show your captures as thumbnails. Open one in a
  lightbox, copy it to the clipboard, save a copy elsewhere, reveal it in its
  folder, or delete it to the recycle bin. A built-in annotator lets you zoom,
  pan, draw markers, erase, and crop, then save the annotated copy.
- **Custom instance pictures.** Upload your own image for an instance and frame
  it with a square pan-and-zoom crop; it then appears as the instance's avatar
  in the sidebar picker, the Overview header, and the manage dialog.
- **Choose where launcher data lives.** A new "Data location" control (Settings
  → Storage) moves your entire data folder to another place — a different drive,
  for example — by copying, verifying, repointing, and restarting, with live
  progress and a "Reset to default". If the chosen location later becomes
  unavailable, the launcher falls back safely instead of losing your data.
- **Hide sidebar buttons you don't use.** Toggle individual secondary sidebar
  buttons in Settings → Appearance, or right-click a button and choose "I don't
  need this" to hide it. You can also open Manage for any profile directly from
  the account dropdown.

### Changed

- **Seamless Windows updates.** Updates now install passively — no UAC prompt —
  and the launcher relaunches itself when they finish, keeping the per-user
  install and leaving your data untouched. The self-update also shows a real,
  determinate download progress bar instead of an indeterminate spinner.
- **Branded, localized Windows installer.** The installer now carries Lucerna's
  lantern artwork and an English / Russian language selector; the unpublished
  MSI target was dropped in favor of the NSIS installer.
- **Readable instance & server folders.** New instances and servers are now
  stored in human-readable folders named after their display name (for example
  `instances/All-The-Mods-10`) instead of an opaque identifier. Existing
  directories are left exactly as they are.

### Fixed

- The Russian "hide this button" confirmation pointed users at a settings
  section name that does not exist ("Внешний вид"); it now names the real one
  ("Оформление").

## [0.16.0] — 2026-07-02

A broad quality, accessibility, and security hardening pass across the launcher.

### Changed

- The improved onboarding tour is shown again to everyone once, and its
  Skip / Back / Next controls now sit consistently on every tour screen.
- Remaining hardcoded English strings (self-update, modpack export, server
  settings, file-dialog filters) are now translated, and several Russian terms
  and plural forms were corrected.
- **Deleting an instance now asks you to type "Delete" to confirm.** Because
  removing an instance destroys all of its worlds, mods, and configs, the
  confirmation is now as strong as the single-world delete dialog.

### Fixed

- Microsoft accounts no longer launch the game with an expired token right after
  opening the launcher, which had caused sporadic "Invalid session" errors when
  joining multiplayer servers.
- Beta Fabric/Quilt loader versions (for example `0.24.0-beta.1`) now install
  and launch instead of failing with a misleading "unsupported Minecraft
  version" error.
- An interrupted download no longer leaves a corrupt file that permanently
  breaks an instance — downloads are now atomic, and a corrupt version cache
  self-heals on the next launch.
- Importing a CurseForge modpack no longer aborts the entire pack when a single
  file has been delisted; that file is reported and the rest still installs.
- Server fixes: restoring a backup at the retention cap no longer deletes the
  snapshot being restored; the "extra JVM args" setting is now actually applied
  to the launched server; and automatic backups resume after a launcher restart.
- Switching instances quickly no longer shows stale data from the previous
  instance (update checks, overview statistics, modpack browsing).
- The onboarding tour can no longer trap the interface behind a dimmed screen.
- Numerous accessibility fixes to the Logs viewer, dialogs, tab panels, memory
  sliders, and tour controls.
- Legacy pre-1.7.3 asset indexes are now materialized, so very old versions
  launch with their sounds and language files.

### Security

- Modpack and world imports now enforce real decompressed-size limits, so a
  malicious archive can no longer exhaust memory or disk (zip-bomb hardening).
- Updated `ammonia` (the HTML sanitizer applied to mod descriptions) and
  `quick-xml` to patched releases that address recently published advisories.

## [0.15.1] — 2026-06-23

### Added

- **In-app self-update on Linux (AppImage).** AppImage builds can now update
  themselves from within the launcher, reusing the same cosign signature
  verification as the Windows updater.
- **Expanded onboarding tours.** New contextual tours for the Servers and
  Add-ons tabs and for log diagnosis, plus main-tour steps covering Quick Play,
  account discovery, and importing a world.

### Changed

- **Honest Linux update fallback for `.deb` / `.rpm` installs.** When an
  in-place update is not possible, the launcher now points to the correct
  package download instead of implying a silent self-update.
- Onboarding copy was reworded for clarity, and the Add-ons and pre-flight
  tooltips were sharpened.

### Fixed

- Orphaned onboarding tour anchors now attach to the correct UI elements.
- The Logs view no longer prints the same crash diagnosis twice: the inline
  per-file diagnosis card no longer repeats the title, explanation, and
  recommendation already shown by the banner above it.
- Corrected the package description shown in Linux software centers.
- Bumped `crypto-bigint` off a yanked release (0.7.4 to 0.7.5).

## [0.15.0] — 2026-06-22

### Added

- **Run your own Minecraft server (Beta).** Create, configure, and launch a
  dedicated server from the launcher across Vanilla, Fabric, Forge, NeoForge,
  and Quilt, with a live console, a one-click "allow offline players", and
  console backfill. Servers can be renamed, edited (memory & JVM args), and
  deleted, and a running server shows an indicator on the sidebar.
- **Server launch-error & crash diagnosis.** Failed starts and silent crashes
  are classified (EULA not accepted, port in use, out-of-memory, corrupt jar,
  missing dependency, world/session lock, and more) with one-click fixes, plus
  dismissible diagnosis banners.
- **Windows Firewall help.** Detects when a server port is blocked and offers a
  one-click allow-rule.
- **Friends can join.** Surfaces the LAN address with copy-invite and clearer
  online-mode messaging.
- **Server backups.** Snapshot, restore, and keep-N backups of your server.
- **Server logs.** Past sessions are rotated, retained, and browsable, with
  one-click share to mclo.gs.
- **Upload your server to a host.** Tracked, cancellable, parallel, and
  resumable SFTP upload with honest byte-based progress and ETA, selective
  upload, a free-space preflight, lock-file exclusions, and a password reveal
  toggle with a save-password opt-out.
- **Import & convert.** Import an existing server from a `.zip` or folder (a
  wizard with drag-and-drop), create a client instance from a server, and have
  client-only mods set aside automatically on create.
- **Import worlds** into an instance from a local `.zip` or folder.
- **Smarter mod dependencies.** Missing dependencies are resolved from the jar
  manifest with one-click install, version incompatibilities are remediated
  inline in the pre-flight panel, and the dependency resolver was reworked to be
  range-aware and to honor author-pinned versions.
- **Wrong-loader jar detection on modpack import.** When a pack bundles a mod
  jar built for a loader family your instance can't load (for example a Fabric
  jar in a Forge instance), the importer now flags it: a completion toast lists
  the affected files, and a "Files that won't load" note in the imported-pack
  drawer keeps them visible after a restart.
- **"Fix available" marker in Logs.** A log file with a diagnosable, fixable
  problem now shows a wrench marker, so you can spot it without opening each
  file in turn.
- **Embedded CurseForge key.** Release builds ship with a CurseForge API key, so
  browsing CurseForge works without entering your own; a curated community fix
  mod can be suggested for known issues (e.g. the Create goggle-overlay spam).
- **Play into a world.** A dropdown on the Play button launches straight into a
  chosen world, with matching green buttons on the Worlds tab.
- **Instance memory controls.** An advanced initial-heap (`-Xms`) setting and a
  shared memory slider with numeric entry, endpoint labels, and a recommended
  marker.
- **Quality of life.** Explicit loading spinners wherever content loads, a
  dismissable "Needs attention" panel with restore, a launcher-wide button
  consistency pass, live progress on modpack updates (with apply from Overview),
  colour-coded server/log status icons on the sidebar, an offline-name dialog,
  and a redesigned app icon.

### Changed

- **One error-display policy** across every surface — no raw URLs or internal
  detail leak into user-facing messages.
- **Accessibility.** WCAG fixes across the manage modal, loader picker, and
  memory slider, plus a live region that announces errors to assistive tech.
- The manage-instances modal was overhauled: list usability, legible auto-save,
  and consistent microcopy.

### Fixed

- Per-host request throttling stops mod-API 429 storms, and installed-mod
  metadata is batched to de-storm the dependency graph.
- Dependency pre-flight is scoped to the instance's loader (no more phantom
  cross-loader requirements) and reads nested Fabric/Quilt jars correctly.
- Non-ASCII offline nicknames that cannot enter Minecraft worlds are now blocked
  at account creation.
- Modals dismiss only on a true click-outside, not a drag-select released on the
  backdrop, and the sidebar no longer scrolls its install footer out of view.
- Various server fixes: CRLF `server.properties` handling, a low-disk advisory,
  and a port-change fix that now takes effect.

## [0.14.0] — 2026-06-17

### Added

- **Real launcher log.** The "Launcher logs" group now captures a genuine
  app-wide launcher log (`lucerna.log`); the game's captured stdout/stderr is
  relabeled "Game console" so the two are no longer confused. Network errors
  (429 / 5xx / unreachable host) are logged centrally at the network chokepoint.
- **Wider import auto-detection.** Importing an existing instance now also
  auto-detects the CurseForge App, ATLauncher, and the Modrinth App, plus a
  Roaming `.minecraft`, with an empty-state when nothing is found.
- **Auto-detect loader on import.** When importing a launcher instance, the mod
  loader is now inferred from the `mods/` folder instead of defaulting to
  Vanilla, with a warning when a Vanilla import still carries mods.
- **Latest-bound crash diagnosis.** Log diagnosis now binds to the most recent
  log, the out-of-memory fix is idempotent (no more repeated doubling), and the
  result surfaces consistently in a banner, a sidebar badge, and Overview.
- **Single-instance guard.** Launching Lucerna while it is already running now
  focuses the existing window instead of opening a second copy.

### Changed

- **Honest CurseForge key errors.** A failing CurseForge key check now
  distinguishes an unreachable host and a region block (Cloudflare) from an
  actually invalid key, instead of always reporting "invalid key".

### Fixed

- Dependency pre-flight now reads Fabric/Quilt nested jars and their `provides`,
  fixing a false "not installed" for bundled dependencies.
- The compatibility-check list scrolls instead of overflowing the window.
- Hash-enrichment no longer backfills a loader/MC-mismatched version.
- The expanded window's minimum-height floor holds from a cold start.
- The onboarding tour's Skip button is a real, properly aligned button with the
  full "Пропустить обучение" label and an even footer gap — no more link-style
  or wrapped text.
- Settings shows a proper ". " separator between the CurseForge replace-key
  action and its hint.

## [0.13.0] — 2026-06-16

### Added

- **Quick Play.** Launch straight into a single-player world or connect to a
  multiplayer server by address, skipping the in-game menus.
- **Saved servers.** Read, add, delete, and copy entries from your server list
  directly in the Connect-to-server flow.
- **Import an existing instance.** Bring instances in from another launcher —
  Prism / MultiMC and a generic `.minecraft`, plus TLauncher and the official
  launcher — and open an imported instance's original source folder.
- **GPU selection.** Prefer a specific GPU for Minecraft, on both Windows and
  Linux.
- **Adaptive per-instance memory.** Heap defaults now scale to the machine's
  physical RAM, with a full-RAM slider and a warning band when you push past a
  safe share.
- **Player profile avatar.** The signed-in account's Minecraft skin head now
  appears in the account selector.
- **Manual install for resource packs and shaders.** Install a local `.zip` for
  resource packs and shaders, matching the existing manual mod install.
- **Dependency version pre-flight.** An offline check flags dependencies that
  are missing or whose installed version falls outside a mod's required range
  before you launch.
- **Server-join repair.** When a modded-server join fails, Lucerna diagnoses the
  log and offers to install missing mods, replace mismatched versions, or
  disable client-only mods the server rejects.
- **Log management.** Delete individual log files, clear old logs, and opt in to
  a retention policy that caps file count and total size.
- **Unified action queue.** Long-running operations — integrity verify/repair
  and modpack import — now run through a single serial queue you can cancel and
  reorder.
- **Confirm before removing an account.** Removing an account now asks first,
  with a per-row delete affordance in the account list.
- **Rainbow icon hover.** The Browse-modpacks and Shaders-tab icons animate
  through a rainbow on hover, with an opt-out toggle in Settings → Appearance.
- **Loader-aware shader hint.** The Add-ons → Shaders hint now detects an
  already-installed shader loader (Iris, Oculus, OptiFine) and hides itself once
  one is present, and points Forge / NeoForge instances at Oculus.

### Changed

- **Settings reorganized.** Settings is now a 7-section vertical-sidebar shell
  (Appearance, Game, Integrations, Storage, Updates, Help, About).
- **Modpack browsing.** Browse cards show the pack author and offer one-click
  install of the latest version, and pack updates are now surfaced across the
  launcher for every source.
- **Unified card language.** Mods, resource packs, shaders, and modpacks share
  one compact card design with consistent actions and context menus.
- **Logs toolbar.** The log panel toolbar was redesigned around an icon reload
  button, an overflow menu, and level-filter chips.
- **Unified tooltips.** A single shared tooltip layer replaces scattered native
  `title=` tooltips.
- **Iconography & sidebar.** Consistent icons across the sidebar buttons, and
  the sidebar mods button now reads clearly as "open folder".
- **Import picker.** The modpack import picker selects all files by default, and
  a pack's bundled resource packs and shaders now show up under Add-ons →
  Installed.

### Fixed

- A maximized window stays maximized across an F5 reload, and browse error bars
  gained a Reload retry.
- The Overview's version-manifest fetch now self-heals — no more stale error
  banner after the machine sleeps and resumes.
- CurseForge search is paged in ≤50-item windows, fixing an HTTP 400 when
  "100 / page" was selected.
- The compact window now fits the status row instead of scrolling.
- The modpacks browser stays open after an install completes.

## [0.12.0] — 2026-06-11

### Added

- **Auto-Repair.** The crash-log diagnoser now goes one step further: detected
  problems (out-of-memory, missing loader, corrupt jar, file conflicts) come
  with a one-click preview → confirm → apply fix.
- **Multi-account sign-in.** Sign in to several Microsoft accounts and switch
  between them, with a clearer sign-in flow and a "Buy Minecraft" hint when an
  account has no Minecraft profile.
- **ATLauncher modpacks.** A fourth modpack source alongside Modrinth,
  CurseForge, and Feed The Beast.
- **Compact launcher mode.** A sidebar toggle shrinks the window down to a
  compact strip for a minimal footprint.
- **Find a blocked mod elsewhere.** When a mod is blocked for download, Lucerna
  can now look it up on Modrinth and offer a working substitute.
- **Proactive incompatible-mod tracking.** An offline pre-scan flags mods that
  likely don't match the instance's loader/version, then auto-confirms suspects
  against the live API.
- **What's new in-app.** Settings → About now shows a collapsible changelog.
- **Clearer dependency errors.** When a required dependency can't be resolved,
  the install dialog now explains why — no version for this Minecraft version,
  the wrong loader, or no published versions at all — instead of only naming it.

### Changed

- **Overview tab redesign.** The Overview is now a structured instance dashboard
  with a status pill, an attention panel, and an update check.
- **Imported-pack detail drawer.** Reworked into collapsible, attention-first
  sections with asset fixes.
- **Iconography.** Unicode glyph indicators across the UI replaced with a
  consistent lucide icon set.
- **Accessibility.** A shared modal primitive with focus-trapping and focus
  restore now backs every dialog, plus assorted a11y fixes.
- **Consistent busy states.** Install / update / uninstall buttons share one
  controlled spinner treatment.
- **Faster installs.** Install-time downloads now run in parallel, and the log
  viewer renders large logs via windowing.
- **Units.** Byte and memory sizes are now formatted consistently and localised.

### Fixed

- A user-requested Stop is no longer reported as a crash on the Overview.
- Oversized modpack overrides are skipped instead of aborting the whole import.
- The Settings modal now layers correctly above the modpacks modal.

### Security

- Reject path-traversal in API-supplied mod filenames.
- SHA-1-verify the Forge installer JAR before caching it, and hard-error on a
  missing Mojmaps SHA-1.
- Harden launch-argument handling (`substitute()`, `max_heap_mb`,
  `extra_jvm_args`).

## [0.11.0] — 2026-06-05

### Added

- **Add-ons browser.** A new Add-ons tab with a Mods · Resource packs · Shaders
  switcher lets you search and install resource packs and shader packs from
  Modrinth and CurseForge — with installed-awareness and Browse ↔ Installed
  sync, the same flow mods already use.
- **Instance integrity — Verify & Repair.** Scan an instance for missing or
  corrupted files and re-download what's broken, with a passive status indicator
  on the Overview tab and a background-operation queue so a scan never blocks
  the UI.
- **FTB modpacks.** Feed The Beast joins Modrinth and CurseForge as a third
  modpack source, with collapsible version groups and the same
  import-to-instance flow.
- **Adjustable explanation depth.** Onboarding and contextual help can now show
  Basic or Advanced detail (Settings → General), so newcomers get plain-language
  guidance while experienced users can skip to the specifics.

### Changed

- **Browse & Installed UX overhaul.** A single mutually-exclusive view filter on
  the Installed tab (All / Enabled / Disabled / Updates / Issues), shared
  pagination across the mod, modpack, and add-on browsers, and accurate Feed The
  Beast pagination totals.

### Fixed

- The Add-ons → Shaders hint is now actionable: it opens an Iris install modal
  or links to OptiFine downloads (naming the active instance's Minecraft
  version), instead of being a dead-end note.
- A healthy ("✓ OK") instance-integrity result from a previous launcher session
  now reads as "Not checked" instead of lingering as stale confidence;
  outstanding problems still persist until you re-verify.
- Switching to a modpack-imported instance no longer falsely raises the
  "detach pack" confirmation when nothing changed; keyring-related text is now
  platform-neutral.
- Russian: the instance-integrity subtitle now reads "профиля" instead of
  "инстанса".

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
- **New app icon.** The placeholder icon was replaced with a custom pixel-art
  lantern matching the Lucerna name, shown in the taskbar, window, and installer.

### Fixed

- Transitive mod dependency resolution now installs dependencies-of-dependencies,
  with a per-mod install toast.
- **Cross-source dependency recognition.** A dependency already installed from
  one platform (for example Balm from Modrinth) is now recognised when a mod on
  the other platform (for example Waystones from CurseForge) requires it, instead
  of attempting a duplicate install that failed with a misleading filename
  conflict. The Installed tab also merges the "has dependencies" and "required
  by" indicators into a single chip with a jump arrow.
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
- Orphaned tray icons no longer pile up. With _hide launcher to tray during
  game_ enabled, each game launch used to leave a tray icon that never went
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

### Changed

- **Reworked mod & modpack browser filters.** Filtering moved into a compact
  toolbar plus a right-side Filters drawer, with active filters shown as
  removable chips, shared across the mod and modpack browsers.
- **Browser polish.** A configurable page size, a grid / list layout toggle,
  aligned toolbar controls, and loading spinners during searches and detail
  loads bring the mod and modpack browsers to visual and functional parity.

### Fixed

- **Changing an instance's Minecraft version or loader now keeps it consistent.**
  The loader version is re-resolved (self-correcting if it went stale), installed
  mods are checked against the new Minecraft / loader combination with a summary,
  an unsupported Forge build surfaces a clear error instead of failing silently,
  and modpack provenance can be detached from the instance.

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

[Unreleased]: https://github.com/AntonBabchenko/Lucerna/compare/v0.17.0...HEAD
[0.17.0]: https://github.com/AntonBabchenko/Lucerna/compare/v0.16.0...v0.17.0
[0.16.0]: https://github.com/AntonBabchenko/Lucerna/compare/v0.15.1...v0.16.0
[0.15.1]: https://github.com/AntonBabchenko/Lucerna/compare/v0.15.0...v0.15.1
[0.15.0]: https://github.com/AntonBabchenko/Lucerna/compare/v0.14.0...v0.15.0
[0.14.0]: https://github.com/AntonBabchenko/Lucerna/compare/v0.13.0...v0.14.0
[0.13.0]: https://github.com/AntonBabchenko/Lucerna/compare/v0.12.0...v0.13.0
[0.12.0]: https://github.com/AntonBabchenko/Lucerna/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/AntonBabchenko/Lucerna/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/AntonBabchenko/Lucerna/compare/v0.9.1...v0.10.0
[0.9.1]: https://github.com/AntonBabchenko/Lucerna/releases/tag/v0.9.1
[0.9.0]: https://github.com/AntonBabchenko/Lucerna/releases/tag/v0.9.0
[0.1.0]: https://github.com/AntonBabchenko/Lucerna/releases/tag/v0.1.0

# Changelog

All notable changes to Lucerna are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Development happens continuously on `main`. Versions between `0.1.0` and the first
published release were untagged feature milestones; the first packaged public
release is **0.9.0**.

## [Unreleased]

### Added

- **Translate the mods you play with.** The Translation screen now lets you write
  your own text for any string a mod shows — the ones nobody has translated, and
  the ones translated badly. Lucerna collects what you write into a resource pack
  and switches it on for you. Your mods are never modified; the pack sits
  alongside them and can be removed at any time.

  Only the strings you change go into the pack, so when a mod author later
  improves their own translation, you get their fix and keep your edits. If a mod
  updates and the original English text behind one of your strings changed,
  that string is flagged as needing another look rather than silently drifting
  out of date.

  Translations are stored per mod rather than per instance, so a string you fix
  once applies to every instance with that mod in it. Applying requires
  Minecraft 1.13 or newer — on older versions a mod's own translations take
  priority over any resource pack, so an override there would have no effect.
  The coverage report still works on those instances.

- **Datapacks, from the catalogue into your worlds.** Instances get a new
  **Data packs** option in Add-ons, alongside mods, resource packs and
  shaders: search Modrinth and CurseForge, install a pack, then choose which
  worlds it goes into. A pack is stored once in the instance's library and
  linked into every world that uses it, so the same pack in five worlds costs
  the space of one. Every library row shows where that pack actually is —
  enabled in some worlds, disabled in others, or in none yet — and expands to
  the per-world detail.

  Updating a pack carries the new version into every world that has it and
  keeps each world's own enabled or disabled setting. Removing one offers to
  clear it out of those worlds in the same step, and reports any world it left
  untouched because the file sitting there isn't the one Lucerna installed. A
  world that loses a pack loses its record of it too, so Minecraft no longer
  asks about a missing data pack the next time that world loads. A world's
  detail view still has its own **Datapacks** tab for working on one world at
  a time, and either place accepts a pack that is a folder rather than a
  `.zip`, or one that was put there outside the launcher.

  Datapacks need Minecraft 1.13 or newer — below that the option isn't
  offered.

- **Vanilla Tweaks, built from inside the launcher.** The datapack views —
  both the instance's and the server's — gain a **Vanilla Tweaks** button that
  opens the same tick-list the site has: every pack in its category, with its
  description and version. Choose what you want, press once, and the packs
  arrive as ordinary datapacks. Packs you already have are marked and their
  boxes are ticked, and when two packs are known not to work together the pair
  is flagged — a warning, not a veto, the same as on the site itself.

  From then on they behave like anything else installed from a catalogue: the
  usual **Check for updates** notices a new version and the usual update button
  applies it. Vanilla Tweaks publishes per Minecraft version, so a version it
  hasn't caught up with yet is reported as exactly that rather than as an empty
  list.

- **Datapacks on your own server.** The server's Add-ons tab gets the same
  **Datapacks** option — install from Modrinth or CurseForge, check for
  updates, and switch packs on and off. On and off are written into the world
  itself rather than faked by renaming files, so the list matches what
  `/datapack list` shows in the server console. Packs you put in the world
  folder by hand — folders as well as `.zip` files — are listed next to the
  ones Lucerna installed. An entry whose file has since gone missing is shown
  as exactly that; clearing it is what stops the server asking about a pack
  that no longer exists. Datapacks are changed while the server is stopped,
  the same rule as its mods and plugins. Servers older than Minecraft 1.13
  don't offer it at all.

### Fixed

- A resource pack could be installed as a datapack on an own-server, and a
  datapack as a resource pack — both are now told apart by their top-level
  folder rather than by `pack.mcmeta` alone.
- World deletion, restore, backup and import are refused while the instance
  is running instead of racing the live game.
- Cloning an instance now carries its datapack library along with it.
- Building a client instance from your own server brings that server's
  datapacks with it. They land in the new instance's datapack library, keeping
  the identity that lets Lucerna offer updates for them later, and you place
  them into a world once you have one.
- The **Details** report after importing a modpack was empty — "0 files" — for
  every pack of about thirty files or more, even though the import itself had
  worked. The list is now taken from the finished operation rather than from a
  progress message that could arrive after it, and shows every file with where
  it came from and whether it was downloaded or reused. Updating a modpack
  gets the same per-file report, which it never had.

## [0.21.0] — 2026-07-31

### Added

- **Switch an installed modpack to any published version.** The imported-pack
  drawer has a new **Change version** action, offered whether or not an update
  is available — reaching an older version while already up to date is the
  point. Pick from the pack's full version list (newest first, filterable by
  Minecraft version, with installed / newer / older badges), then review what
  the switch will do before confirming: the risks of that direction, the
  added / updated / removed file list, and the changelog — which swaps its
  ends on a downgrade, so it shows what you lose instead of framing it as
  what's new.
- **A per-instance activity journal.** The logs window has a new **History**
  view listing what the launcher did to that instance: mods and packs
  installed, updated, removed, enabled or disabled, integrity repairs, and
  finished launch attempts with their outcome. When a log carries a diagnosis
  or is a crash report, the viewer shows what changed in the 24 hours before
  it — the question a diagnosis alone can't answer ("it started after I
  installed X"). Writing the journal can never break the install, launch or
  repair that produced the entry. Separately, runs of three or more identical
  log lines now collapse into one row with an expandable ×N chip.
- **Skin library.** Saved skins in a thumbnail grid with one-click switching
  and a cape remembered per skin, reachable from both the skin & cape manager
  and the pixel editor.
- **Clone an instance.** From the sidebar right-click menu or the Manage
  window, with per-content choices — worlds, mods, resource packs, shaders,
  configs, screenshots — so a clone can carry the setup without dragging the
  saves along.
- **Import from X Minecraft Launcher and Legacy Launcher.** Both are detected
  next to the launchers already supported, with their own badges in the import
  dialog. Legacy Launcher (llaun.ch) installs used to be mislabelled as the
  official launcher — its profile file differs from TLauncher's only in case
  and spelling, and the two are now told apart even when they share a folder.
- **The same mod jar is now stored once and shared between instances.**
  Installing a mod that ten instances already have costs the space of a link,
  not ten copies. Every write into an instance's content folders goes through
  a single chokepoint that writes a fresh file and renames it into place, so a
  shared jar can never be modified underneath the instances using it. Integrity
  records also survive a jar whose bytes changed instead of quietly demoting a
  known mod to an unknown local file.
- **Pick the memory when creating an instance.** The create form now carries
  the same memory slider as the instance detail editor, seeded from the
  adaptive default for your machine. Previously the heap was only editable
  after the instance already existed.
- **Update all** for resource packs and shaders in the Installed view, with
  per-row updates that replace the old file instead of leaving both versions
  installed.
- **The Minecraft EULA is readable before you accept it.** The agreement's
  name is now a link — in the own-server create wizard, in the server import
  view, and beside the diagnosis banner's one-click accept — so the document
  is no longer reachable only after consenting to it. The text is deliberately
  not bundled: it is Microsoft's document and changes without our involvement,
  so an embedded copy would eventually present a stale revision as the
  agreement in force.
- **Import a modpack from a link, and one-click shortcuts on your desktop.**
  Paste a Modrinth or CurseForge modpack page link into Modpacks → "Import from
  URL…" and you land in the usual import flow — pack details, pick a version,
  choose the files. A website or another app can hand Lucerna a `lucerna://`
  link too, but a link can only ever *open* that dialog: it can never install a
  pack or start the game on its own, and Lucerna says so when a link arrived
  from somewhere else. Handling those links is off until you switch it on in
  Settings → Integrations, which names the exact registry key it writes and
  removes it again when you switch it back off. Separately, you can now create a
  desktop shortcut for an instance — or straight into a specific world or onto a
  saved server (Minecraft 1.20+) — from the instance right-click menu or the
  Manage window. Link handling is Windows-only for now; shortcuts also work on
  Linux.
- **Saved-server status, off until you ask for it.** Settings → Game has a new
  "Show status for my saved servers" permission, off by default. Turn it on and
  the servers list shows each saved server's player count, version and response
  time, with its MOTD on hover. Leave it off and Lucerna sends nothing to those
  hosts — and says so in the list instead of showing blanks. The permission is
  enforced by a new single-file network chokepoint that physically cannot dial
  without it (re-read from disk on every check, so switching it off takes effect
  immediately), capped at four connections at a time and bounded by timeouts,
  with a structural test that fails the build if any of that is bypassed. The
  setting spells out the trade-off in plain words: those servers see your IP
  address, the same as when you join them. A server that does not reply is
  reported as "no answer" rather than "offline" — Lucerna does not yet follow
  SRV records, so silence is not proof the server is down.

- **Everything lives in the install folder now.** On a fresh start, Lucerna
  creates its data root (`LucernaData` — instances, worlds, mods, caches,
  Java runtimes, logs, and in release builds the embedded browser profile)
  right next to the executable instead of hiding it in `%APPDATA%`. A
  `LucernaData` folder already sitting next to the executable is adopted
  automatically when it actually looks like Lucerna data (an unrelated folder
  that merely shares the name is left untouched) — so uninstalling while
  keeping your data and later reinstalling into the same folder reattaches
  everything, no configuration involved. Existing installs with data in
  `%APPDATA%` and explicitly
  relocated roots keep working exactly as before; unwritable install
  locations fall back to `%APPDATA%`.
- **The Windows uninstaller now says what stays behind — and can remove it.**
  Uninstalling used to delete only the application: the data root (instances,
  worlds, servers, mods — often gigabytes, and invisible to the uninstaller
  when relocated), saved account sign-ins in Windows Credential Manager, and
  orphaned uninstallers from older versions all survived silently. The
  uninstaller now lists every directory it found — game data, launcher
  settings and logs, browser cache, leftovers of older installers — with its
  path and size, plus how many saved sign-ins sit in Windows Credential
  Manager, in the uninstaller's own language, and asks once whether to erase
  it all (keeping it stays the default answer); agreeing also clears the
  credential-manager entries. Data next to the executable is found even when
  the location pointer is broken or lost. Silent, passive and update runs
  never prompt and never delete data. A data root that is not reachable at
  uninstall time (for example on an unplugged drive) is never deleted, and its
  location pointer is kept so the data stays discoverable.

### Changed

- **The Manage window now uses the whole window.** The instance list and the
  detail pane are split by a handle you can drag — the list may grow until the
  form would lose its comfortable width, instead of stopping at a fixed point
  with a third of a wide window sitting unused. The instance picture is shown
  where you change it (it used to be edited blind, visible only after closing
  the modal), and the same picture control is now used on the Overview header
  and in the Manage window. On a wide pane the form splits into two columns —
  what you *set* on the left (name, version, loader, memory), what you
  *inspect* on the right (advanced heap, JVM arguments, provenance,
  integrity); below that width it stays a single centred column. The action row
  is a pinned footer, so Close never falls below the fold.
- **The Overview cards are click targets.** Clicking the Configuration, Mods or
  Integrity card opens the Manage window on the matching section and briefly
  flashes the field it was opened for, instead of asking you to find a small
  button first.
- **Modpacks: it now says up front what an install will create.** The browser's
  header and the text above the install button state that importing creates a
  new instance, so the scope is visible before you commit. The filter row and
  the pagination footer stay pinned while the results scroll — in all five
  paged browsers — and the modpack browser uses the full window.
- **Installing mods is now all-or-nothing.** A batch that fails partway rolls
  back the jars it already wrote and the registry entries it already added,
  instead of leaving a half-installed set behind. The failure toast names the
  cause and offers **Retry**.

### Fixed

- Ticking the uninstaller's "Delete the application data" checkbox while
  keeping game data no longer orphans a relocated data root: the
  `data-location.json` pointer is restored, so a reinstall finds the data
  again.
- Changing the data location to a folder that already contains Lucerna data no
  longer nests a fresh `LucernaData` folder inside it and silently abandons the
  existing data. The launcher now recognizes an existing data root (picked
  directly or via its parent folder) and offers to switch to it in place —
  nothing is copied or deleted, and the dialog says where the current data
  stays. Picking a folder already named `LucernaData` no longer doubles the
  subfolder, and picking the current data folder now says so plainly instead
  of failing with a confusing error.
- A data location pointing at a folder that will never come back no longer
  locks the launcher out of recovery. While running from the fallback
  location, **Reset to default** stays available: it removes the pointer only —
  nothing is copied and nothing is deleted — and after the restart the
  launcher picks up the data sitting next to the executable again.
- Applying a modpack update no longer wipes the notes recorded at import
  (oversized overrides that were skipped, files that could not be resolved,
  jars that won't load with the pack's loader) while the files those notes
  describe are still on disk.
- Updating a modpack now keeps mods you had disabled disabled, and no longer
  leaves orphaned `.disabled` files behind. Jars a pack ships for the wrong
  loader can be disabled in one click from the import drawer.
- The proactive incompatible-mod scan now also judges the jars a modpack
  brought with it, not only mods you installed yourself.
- The modpack import picker labels the files a pack marks as optional, and the
  FTB browser's Minecraft-version filter now matches any version of a pack
  rather than only its newest one.

## [0.20.0] — 2026-07-27

### Added

- **One-click Optimise.** A new **Optimise** button on an instance's Overview
  installs a curated, loader- and version-aware set of performance mods (Sodium,
  Lithium, Embeddium, ImmediatelyFast, Entity Culling, FerriteCore, Dynamic FPS
  — resolved live against the instance's loader and Minecraft version). It
  previews exactly what will install, skips mods you already have, avoids the
  rendering optimizer when OptiFine is present, and installs the rest through the
  normal dependency-aware pipeline. Disabled on vanilla instances (no mod loader).
- **Log file actions where the files are.** In the logs viewer, every file row
  now carries its own actions: **Share** and **Open folder** appear on hover
  next to the delete button, and a right-click (or Shift+F10) menu offers
  Share / Open folder / Delete. Sharing a file you don't have open first opens
  it in the viewer, so you always see exactly what will be uploaded before the
  anonymised mclo.gs confirm step. The header's ⋯ menu is gone — **Clear old**
  is now a direct button in the toolbar, and the logs tour was updated to match.
- The sidebar **Skins button can now be hidden**, like the other secondary
  buttons — right-click it, or toggle "Skins and capes" in Settings →
  Appearance. Both variants (Skin & cape manager and the pixel editor) respect
  the setting.

### Changed

- **Sidebar lower zone regrouped.** Browse modpacks / Import / Gallery / Logs /
  Settings now sit in labelled **Content** and **View** sections (with a
  heading-less Settings footer), with unified button sizing and consistent
  spacing that matches the Account and Profile sections.

### Fixed

- Menus opened with the mouse no longer pre-highlight their first item (a
  one-item right-click menu used to render as a single "selected" row).
  Keyboard opens — Shift+F10, the menu key, or activating a ⋯ trigger with
  Enter — still start with the first item highlighted, per OS convention.
- The log-share anonymiser now also scrubs Linux `/home/<user>/` paths and home
  paths that end exactly at the username (e.g. `HOME=/home/player` at
  end-of-line), on every OS, so those usernames no longer reach a shared
  mclo.gs paste.
- The modpack import picker's "may prevent the pack from launching" warning is
  now scoped to mods only — deselecting resource packs, shaders, or config no
  longer triggers it.
- One press of Escape no longer closes both the screenshot lightbox and the
  gallery behind it — the lightbox now participates in the shared topmost-only
  Escape handling.
- Server hot backups no longer race the save: the backup waits for the server's
  "Saved the game" confirmation (instead of a fixed delay) before zipping, so a
  backup taken while the server runs can't capture a torn world snapshot.

### Security

- Server Forge/NeoForge installer downloads are now SHA-1-verified against the
  Maven checksum sidecar, matching the client-side installer path (they were
  previously fetched without checksum verification).
- Bumped `ammonia` 4.1.3 → 4.1.4 for RUSTSEC-2026-0213.

## [0.19.0] — 2026-07-16

### Added

- **Run several instances at once.** Minecraft is no longer one-at-a-time: you
  can launch different instances concurrently — for example a modded world on
  one account and a vanilla world on another — and each keeps its own playtime.
  The Play/Stop button follows the selected instance, the sidebar shows a
  per-instance running badge with an inline Stop, and the Client / Servers
  switcher carries a running-count badge that opens a popover to stop, restart,
  or jump to any running client or server. Launching warns (without blocking) if
  the combined memory reservation would over-commit your RAM, or if two copies
  would share one account; starting the *same* instance twice is blocked with a
  clear message. English and Russian throughout.
- **Server Add-ons reach client parity.** The Servers → Add-ons → Installed pane
  now renders enriched cards (icon, name, version) for both mods and plugins
  instead of a bare filename list, with enable / disable / delete and a details
  view. It gains **update-checking** — a scan, a per-row **Update**, and **Update
  all** — for server *mods* (Fabric/Quilt/Forge/NeoForge) and server *plugins*
  (Paper/Purpur) alike, plus search / enabled / disabled / sort filters. The
  Browse side now shows what is already installed instead of offering to
  re-install it, with a **Show installed** toggle. Plugins hosted externally on
  Hangar open their project page to download (and are skipped by Update all),
  matching the plugin browser.
- **In-launcher changelog for updates.** Wherever an update is offered — an
  installed mod, a resource pack or shader, or an imported modpack — a 📜
  **Changelog** affordance lets you read what changed before applying it,
  cumulatively across every version between the one you have and the update
  target. Works for Modrinth and CurseForge sources.
- Server Settings: the `server.properties` block is now a full, searchable
  editor covering every vanilla key, each with an inline description and its
  default value.
- **Inline hints in logs.** Well-known errors (mod conflicts, memory, drivers,
  network, world corruption, and more) get a marker in the log viewer and the
  server console; hovering the line shows what the error means and how to fix
  it, in English and Russian.
- **Skin editor: symmetry mirror and viewport upgrades.** The editor gains a
  geometrically-correct left↔right body mirror (paint one arm and the other
  follows), a resizable 2D texture panel, static pose presets (Default / T-pose
  / Walk / Sit), odd-sized brushes centred on the cursor with a live hover
  footprint shown on both the 2D texture and the 3D model, and companion
  zoom / pan. Saving now strips stray pixels outside the UV layout.
- **Skin editor: an editable colour palette.** The fixed palette is replaced by
  one you fully control — add the current colour, edit a swatch in place,
  reorder by drag or keyboard, and delete — with the palette persisted between
  sessions and a reset back to the defaults.
- **Skin editor without a Microsoft account.** Offline and no-account users can
  open the pixel skin editor and export a PNG; only uploading the result to a
  Minecraft profile still requires a Microsoft sign-in.
- **Quick access to a server's add-on folder.** The Servers-mode sidebar gains a
  button that opens the selected server's `mods/` (Fabric/Quilt/Forge/NeoForge)
  or `plugins/` (Paper/Purpur) folder in the OS file manager; it is hidden for
  vanilla servers.
- **Force stop for servers.** When a graceful stop drags on — a still-loading or
  hung server that isn't responding to the shutdown command — a **Force stop now**
  button appears under Stop after a few seconds, ending the server immediately
  instead of waiting out the graceful-shutdown window. A force-stopped server is
  reported as stopped, not crashed.

### Changed

- The skin & cape dialogs now fit smaller, non-maximized windows (a bounded
  height with a scrolling body) and use compact cape tiles.
- Deleting a world now matches deleting a server — a single confirmation dialog,
  without the extra type-"Delete" step. Whole-instance deletion keeps its
  stronger inline confirmation.
- The out-of-place "Allow offline players" button was removed from the server
  Connect card, where it read as a network toggle; online-mode is still toggled
  from Server Settings.

### Fixed

- Stopping a server before it finished loading no longer falsely reports that
  "a client-only mod crashed the server". A server you stop — including one the
  launcher force-kills after the graceful-shutdown window times out — is now
  reported as stopped, not crashed.
- The server Add-ons → Installed pane no longer blanks out and reloads for
  several seconds each time you switch between Browse and Installed: it stays
  mounted, shows a spinner while loading, and caches file hashes between lists.
- In Servers mode the sidebar no longer shows the client-only Account section or
  the instance Logs button, neither of which applies to a dedicated server.
- Screen readers now announce the body text of a confirmation dialog, and the
  Minecraft-version picker reports the committed selection rather than the
  keyboard-highlighted row.

## [0.18.0] — 2026-07-12

### Added

- **Servers are now a first-class mode, not a modal.** A **Client / Servers**
  switcher sits under the sidebar header. In Servers mode the sidebar mirrors
  the client — a server selector with live status icons, a create button, and a
  large Start/Stop — while the right panel hosts the full server management UI.
  The switcher itself carries live status for both sides: the Servers segment
  pulses while a server runs (and flags crashes or pending fixes), and the
  Client segment now pulses green while Minecraft runs and turns red after a
  crash, so a crash that happens while you are in Servers mode no longer goes
  unnoticed. Your last-used mode and selected server are remembered between
  launches.
- **Bukkit-plugin server cores — Paper and Purpur.** Create, provision, and
  launch a Paper or Purpur server; friends join with a plain vanilla client, no
  client-side install required. A new **Plugins** area browses plugins from
  Modrinth and Hangar, installs a chosen version (not just the newest), and
  enables, disables, deletes, or reveals them on disk; local `.jar` files are
  validated before installing. Switching an existing server between Vanilla,
  Paper, and Purpur takes a mandatory fresh backup first and swaps the core
  atomically. English and Russian throughout.
- **Pixel skin editor.** From the Skin & Cape dialog, "Edit skin" opens an
  editor that paints directly on the rotatable 3D player model, with a
  synchronized 2D atlas companion for pixel-precise work and occluded faces.
  Tools include pencil, eraser, eyedropper, face-bounded fill, dodge/burn, and
  noise, with per-stroke undo/redo, mirror-X, a palette and custom colours,
  base/overlay layers, classic/slim models, and PNG import/export. Microsoft
  accounts apply the result straight to their profile; offline accounts export
  it.

### Changed

- **Server management regrouped into five tabs.** The eight flat server tabs
  become **Overview / Settings / Add-ons / Hosting / Backups**, mirroring the
  client layout. Overview gathers server facts, LAN and invite connection
  details with a one-click firewall rule, and the full console; Settings
  collects launch config, `server.properties` (a curated form plus a raw
  editor), the core switch, and the danger zone; Add-ons brings client-parity
  Browse/Installed sub-tabs with sort and grid/list toggles and drag-and-drop
  install. The server content browser also gained an Overview tab with the
  project description and image gallery.
- **3D skin & cape controls.** Both the preview and the editor now support
  right-drag to rotate and middle-drag to pan (left-drag still rotates the
  preview and paints in the editor), and the editor's 3D viewport is resizable
  via a draggable splitter that keeps the model crisp — which also fixes the
  fullscreen toggle.

### Fixed

- The instance list no longer takes several seconds to appear after a restart
  when the data folder is large: the folder-size calculation that used to run
  (and block other startup requests) on every launch now runs only when
  Settings → Storage is opened. While the list is loading, the sidebar shows a
  small spinner instead of a misleading "No instances yet".
- Tooltips triggered by keyboard focus now appear only on real keyboard focus
  (`:focus-visible`), so they no longer flash when a modal opens or closes.
- A custom instance picture now displays correctly in the packaged app: it is
  loaded through a `data:` URL so the production content-security policy no
  longer blocks it.
- The boxed segmented control now shows a clearly visible active state for the
  selected segment.
- The saved-servers list shows the source instance's name in a row's subtitle
  instead of a raw internal id.
- Creating, importing, starting, or restarting a server is now safely rejected
  when the data location has fallen back to its default, so writes never land in
  the wrong place.
- Russian text for "installed with dependencies" now uses correct plural forms.

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

[Unreleased]: https://github.com/AntonBabchenko/Lucerna/compare/v0.21.0...HEAD
[0.21.0]: https://github.com/AntonBabchenko/Lucerna/compare/v0.20.0...v0.21.0
[0.20.0]: https://github.com/AntonBabchenko/Lucerna/compare/v0.19.0...v0.20.0
[0.19.0]: https://github.com/AntonBabchenko/Lucerna/compare/v0.18.0...v0.19.0
[0.18.0]: https://github.com/AntonBabchenko/Lucerna/compare/v0.17.0...v0.18.0
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

# macOS Beta Testing Checklist

Lucerna's macOS build is **beta**: CI compiles it, runs the Rust suite on a
real Mac, and publishes a Universal2 (Apple Silicon + Intel) `.dmg`. What CI
*cannot* do is launch Minecraft. This checklist is the deferred end-to-end
verification — run it on real hardware and report results on the tracking
issue.

The `.dmg` is **unsigned** (ad-hoc only, not notarized), so Gatekeeper
quarantines a freshly downloaded copy. That is expected; the install step
below clears it.

## Before you start

- Note your hardware: **Apple Silicon** (M-series) or **Intel**. Test on
  whichever you have; both are covered by the single Universal2 artifact.
- Note your macOS version.

## 1. Install

- [ ] Download `Lucerna_<version>_universal.dmg` from the GitHub Release.
- [ ] (Recommended) Verify it with cosign using the command in the release
      notes, and check it against `SHA256SUMS`.
- [ ] Open the `.dmg` and drag **Lucerna** to `/Applications`.
- [ ] Clear the quarantine flag, then launch:
      `xattr -dr com.apple.quarantine /Applications/Lucerna.app`
      — or right-click the app → **Open** → **Open** on the first run.
- [ ] The launcher window opens (820×520) without a "damaged / can't be
      opened" error.

## 2. Accounts

- [ ] Create an **offline** account; it appears in the account switcher.
- [ ] "Sign in with Microsoft" completes the OAuth flow and the account
      appears in the switcher — it does not crash.

## 3. Download a version + JRE

- [ ] Create an instance and pick a Minecraft version.
- [ ] The correct **Java runtime downloads from Mojang** (no system Java
      required) — watch for the JRE download progress.
- [ ] No "unsupported platform" / symlink / permission errors during the JRE
      install (the `mac-os-arm64` / `mac-os` JRE and any manifest symlinks
      resolve).

## 4. Launch Minecraft (the real signal)

- [ ] Click **Play**. Minecraft launches and reaches the main menu.
- [ ] On Apple Silicon the game runs (the JRE arch matched the host slice).
- [ ] Click **Stop** (or quit Minecraft): the process is terminated and the
      launcher returns to a stopped state — no orphaned `java` process is
      left running (check Activity Monitor).

## 5. System integration

- [ ] Enable Settings → Game → "hide launcher to tray when Minecraft
      starts". On the next launch the launcher hides (it hides **immediately**
      on macOS — window-ready detection is a deferred follow-up).
- [ ] On game exit the launcher restores.
- [ ] Theme picker (light / dark / system), under Settings → Appearance,
      follows macOS appearance.

## 6. Loaders & mods (optional, time permitting)

- [ ] Install a Fabric (or other) loader instance and confirm it launches.
- [ ] Browse + install a mod from the mod browser.

## Reporting

For each section note PASS / FAIL, your hardware + macOS version, and paste
any error text or a crash log (Logs tab → Share). File results on the macOS
beta tracking issue.

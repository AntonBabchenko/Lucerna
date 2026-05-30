# UI testing

FTlauncher uses two complementary layers of UI regression testing.

## Layer 1 — vitest class-string assertions

For "intent" regressions like "the Install button accidentally became
`.btn-secondary` instead of `.btn-primary`". Cheap, fast, runs in
`pnpm test`.

**Custom matchers** (live in [`tests/test-utils/button-matchers.ts`](../tests/test-utils/button-matchers.ts)):

```ts
expect(installBtn).toHaveBtnVariant('primary');
expect(installBtn).toHaveBtnSize('lg');
expect(tab).not.toHaveBtnVariant('secondary'); // negative for tabs
```

**Adding a new intent-critical button.** When you add a new prominent
button in `src/`, add a one-line assertion in the relevant
`tests/button-intents-*.test.ts` file. Pick the variant per the
intent table in
[`docs/superpowers/specs/2026-05-27-button-system-migration-b2a-design.md`](superpowers/specs/2026-05-27-button-system-migration-b2a-design.md).

## Layer 2 — Playwright visual snapshots

For "render" regressions like "the toast background went translucent".
Runs in `pnpm test:e2e`. Baselines are PNGs committed under
`tests-e2e/visual/*-snapshots/`.

**Updating baselines after an intentional visual change:**

```powershell
pnpm test:e2e:update
```

Eyeball each diff PNG before committing the new baseline. Don't
update baselines blindly — that defeats the point of the test.

**Visual tests fail on Windows.** Anti-aliasing and font rendering
differ across OS. Visual snapshots pin to Linux (CI runs them on
Ubuntu). Windows local runs include `test.skip(process.platform !==
'linux', ...)` so they no-op cleanly. If a Windows-local
`pnpm test:e2e` shows skipped visual tests, that's the expected
behavior.

**The CI uploads diff PNGs as artifacts on visual-test failure.** When
a PR breaks a baseline, click the failed `frontend` job → Artifacts →
download `playwright-report.zip`. The HTML report shows the
expected / actual / diff for every failed test.

## Adding a new visual surface

1. Add a `.spec.ts` under `tests-e2e/visual/`.
2. Mock IPC via the [`mock-ipc`](../tests-e2e/helpers/mock-ipc.ts) helper.
3. Set theme via the [`theme`](../tests-e2e/helpers/theme.ts) helper.
4. Use `toHaveScreenshot('descriptive-name.png')` for each frame.
5. Run `pnpm test:e2e:update` once to seed baselines.
6. Verify each baseline PNG visually before committing.

## Out of scope

- Cross-browser testing — Tauri webview is Edge WebView2; Chromium is
  close enough.
- Component-isolated tests (Storybook) — overhead doesn't fit a
  maintainer-only project.
- External visual-diff services (Chromatic, Percy) — repo PNGs are
  sufficient.

## Related

- [Cluster D design](superpowers/specs/2026-05-27-ui-testing-infrastructure-d-design.md)
- [Cluster B2a design (button-system intent table)](superpowers/specs/2026-05-27-button-system-migration-b2a-design.md)
- [`PRINCIPLES.md`](PRINCIPLES.md)

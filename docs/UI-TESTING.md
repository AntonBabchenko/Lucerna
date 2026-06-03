# UI testing

Lucerna uses two complementary layers of UI regression testing.

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
`tests/button-intents-*.test.ts` file. Pick the variant that matches the
button's intent, consistent with the existing assertions there.

## Layer 2 — Playwright visual snapshots

For "render" regressions like "the toast background went translucent".
Runs locally via `pnpm test:e2e`. The spec files live under
`tests-e2e/visual/`; baseline PNGs (`*-snapshots/`) are **not yet seeded
or committed** — see the note below.

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

**The CI visual job is currently disabled.** It is gated off in
`.github/workflows/ci.yml` pending committed Linux baselines, and the
`frontend` job runs only typecheck + `i18n:keys:check` + `pnpm test`
(not `test:e2e`). Once baselines are seeded on Linux and committed, the
job can be re-enabled to upload diff PNGs as a `playwright-report.zip`
artifact on failure. Until then, visual regressions are caught only by a
local `pnpm test:e2e` run on Linux.

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

- [`PRINCIPLES.md`](PRINCIPLES.md)

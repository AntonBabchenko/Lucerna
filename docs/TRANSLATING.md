# Translating Lucerna

Lucerna's interface is fully translatable. English is the source language;
Russian ships alongside it. Additional languages are welcome — you do **not**
need to write any code.

## The easy way — Weblate (recommended)

Lucerna uses [Weblate](https://weblate.org/) for community translation (the
same platform Prism Launcher uses). Everything happens in your browser:

1. Open the [Lucerna project on Hosted Weblate](https://hosted.weblate.org/engage/lucerna/).
2. Sign in (one click via GitHub).
3. Pick your language, or "Start new translation" to add one.
4. Translate strings. You see the English source plus context; Weblate checks
   that placeholders like `{count}` are preserved.
5. Weblate opens a pull request to this repository. A maintainer reviews and
   merges it. After the next build, your language appears in
   **Settings → Appearance → Language** automatically.

Partial translations are fine for a **new** language — any untranslated string
falls back to English.

Russian is the exception: because it ships alongside English, `tests/i18n-parity.test.ts`
requires `ru.json` to carry exactly the same keys as `en.json`, with no empty values
and matching `{placeholders}`. A partial `ru.json` fails `pnpm test` and CI.

## The manual way — edit JSON + PR

1. Copy `src/lib/i18n/locales/en.json` to `src/lib/i18n/locales/<code>.json`
   (BCP-47 code, e.g. `de.json`, `pt-BR.json`).
2. Translate the values. Keep the keys and any `{placeholders}` unchanged.
3. Open a pull request. The file is auto-discovered — no registration needed.

> If you edited `en.json` (rather than only adding a new language file), run
> `pnpm i18n:keys` to regenerate `src/lib/i18n/keys.generated.ts` and commit the
> result — CI runs `pnpm i18n:keys:check` and fails if it is stale.

> Add a display label for your language in `LOCALE_LABELS` in
> `src/lib/settings/AppearancePanel.svelte` (otherwise the picker shows the raw
> code). Optional but nice.

## Status

Lucerna is live on
[Hosted Weblate](https://hosted.weblate.org/projects/lucerna/) — both the
Weblate route and the manual JSON + PR route work. Translations submitted via
Weblate reach the repository as pull requests for a maintainer to review.

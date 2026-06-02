# Translating Lucerna

Lucerna's interface is fully translatable. English is the source language;
Russian ships alongside it. Additional languages are welcome — you do **not**
need to write any code.

## The easy way — Weblate (recommended)

Lucerna uses [Weblate](https://weblate.org/) for community translation (the
same platform Prism Launcher uses). Everything happens in your browser:

1. Open the Lucerna project on Hosted Weblate. *(Link added once the project
   is approved — see "Status" below.)*
2. Sign in (one click via GitHub).
3. Pick your language, or "Start new translation" to add one.
4. Translate strings. You see the English source plus context; Weblate checks
   that placeholders like `{count}` are preserved.
5. Weblate opens a pull request to this repository. A maintainer reviews and
   merges it. After the next build, your language appears in
   **Settings → Appearance → Language** automatically.

Partial translations are fine — any untranslated string falls back to English.

## The manual way — edit JSON + PR

1. Copy `src/lib/i18n/locales/en.json` to `src/lib/i18n/locales/<code>.json`
   (BCP-47 code, e.g. `de.json`, `pt-BR.json`).
2. Translate the values. Keep the keys and any `{placeholders}` unchanged.
3. Open a pull request. The file is auto-discovered — no registration needed.

> Add a display label for your language in `LOCALE_LABELS` in
> `src/lib/settings/GeneralPanel.svelte` (otherwise the picker shows the raw
> code). Optional but nice.

## Status

The Hosted Weblate instance is not connected yet. Until then, use the manual
JSON + PR route above.

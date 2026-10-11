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
falls back to English. The language picker names every language in that
language, and one that is not complete says how much of it is done
("Deutsch · 42% translated"), so nobody picks it expecting a whole interface.
On **System**, Lucerna switches to the system's language by itself once at
least 90% of its strings are translated; below that, anyone can still choose
it by hand.

Russian is the exception: because it ships alongside English, `tests/i18n-parity.test.ts`
requires `ru.json` to carry exactly the same keys as `en.json`, with no empty values
and matching `{placeholders}`. A partial `ru.json` fails `pnpm test` and CI.

## The manual way — edit JSON + PR

1. Copy `src/lib/i18n/locales/en.json` to `src/lib/i18n/locales/<code>.json`
   (BCP-47 code, e.g. `de.json`, `pt-BR.json`; Weblate's `pt_BR.json` spelling
   works too).
2. Translate the values. Keep the keys and any `{placeholders}` unchanged.
   **Delete every key you have not translated yet** (or leave its value
   empty). A value left in English counts as translated, so a half-done copy
   of `en.json` would claim to be complete — and System would pick it.
3. Open a pull request. The file is auto-discovered — no registration needed.

Lucerna uses the canonical form of the code: `pt_BR.json` is `pt-BR`, and an
old alias becomes its current code (`iw.json` is Hebrew, `he`). Two files for
one language fail the tests.

> If you edited `en.json` (rather than only adding a new language file), run
> `pnpm i18n:keys` to regenerate `src/lib/i18n/keys.generated.ts` and commit the
> result — CI runs `pnpm i18n:keys:check` and fails if it is stale.

## Translating the changelog

The launcher shows its changelog — Settings → Updates → **What's new**, and the
dialog after an update — in the interface language. The English source is
`CHANGELOG.md` in the repository root; translations are one Markdown file per
language at `src/lib/changelog/locales/<code>.md`, using the same BCP-47 codes
as the UI dictionaries (`ru.md` next to `ru.json`).

A translation is a mirror of `CHANGELOG.md` in the same
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) shape:

- Keep every `## [version] — date` heading and the link-reference block at the
  end exactly as in English; the launcher matches versions by that label and
  takes dates and links from the English file.
- Keep the same sections in the same order and the same number of bullets in
  each — a version whose counts differ is shown in English, because bullets
  are paired by position. Section headings themselves (`### Added`) may be
  translated; the launcher labels sections from the English kind.
- Keep inline `code` spans verbatim and keep a bullet's **bold lead-in** where
  the English one has it.

A translation may be partial. A version that is missing, or a bullet left
identical to its English text, is shown in English with a short note saying
so; a language with no file at all gets one note for the whole panel. Nothing
is hidden, and nothing is guessed. Translate the newest versions first — they
are what the post-update dialog shows — and back-fill older ones as you like.

Russian is the exception again: `tests/changelog-parity.test.ts` requires
`ru.md` to mirror `CHANGELOG.md` completely, with every bullet translated. A
changelog entry and its Russian twin land in the same pull request.

Weblate: the changelog is intended to become a second component (Weblate's
*Markdown* format, file mask `src/lib/changelog/locales/*.md`, base file
`CHANGELOG.md`, *Deduplicate identical strings* enabled so entries keep their
translation when a release pushes them down the file, Russian excluded by the
language filter). Until that component exists, translate the changelog the
manual way: copy `CHANGELOG.md` to `src/lib/changelog/locales/<code>.md`,
translate, and open a pull request.

## Status

Lucerna is live on
[Hosted Weblate](https://hosted.weblate.org/projects/lucerna/) — both the
Weblate route and the manual JSON + PR route work. Translations submitted via
Weblate reach the repository as pull requests for a maintainer to review.

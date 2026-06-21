# Design

This document is the single source of truth for Lucerna's UI surface: the design tokens, the component vocabulary, and — crucially — *which* element or variant to reach for *when*. It is descriptive of what the code actually does, not aspirational. The implementation source of truth is [`src/app.css`](../src/app.css) (tokens, `@layer base` theming, the `.btn-*` / icon / filter-control utilities) plus the shared primitives under `src/lib/ui/`. Button *intent* is not just convention — it is enforced by tests: the `toHaveBtnVariant` / `toHaveBtnSize` matchers in [`tests/test-utils/button-matchers.ts`](../tests/test-utils/button-matchers.ts) fail the build when a button's variant or size drifts from its intent (see §15).

Where this doc and the code disagree, fix the code or update this file in the same PR — same rule as `docs/PRINCIPLES.md`.

## 1. Foundations

Every colour is a CSS custom property defined once in `src/app.css`: a light `:root` block and a `.dark` class override (Tailwind `darkMode: 'class'`). Tokens are stored as a **bare space-separated RGB triple** — `R G B`, no `rgb()` wrapper, no commas — so the same token works two ways:

- **Raw CSS:** `color: rgb(var(--danger));`
- **Tailwind, with arbitrary opacity:** `bg-danger/10`, `border-warning-text/30` (Tailwind splices the alpha into `rgb(var(--token) / <alpha-value>)`).

When you add a token: store the bare triple, register it in `tailwind.config.cjs` under `colors` (surface/border/accent/state) or `textColor` (text), and reference it as `rgb(var(--x))` in raw CSS.

| Token | Light | Dark | Meaning / when to use |
|---|---|---|---|
| `--bg-base` | `250 250 250` (neutral-50) | `23 23 23` (neutral-900) | Outermost page/app shell background (`bg-base`). Dark is deliberately one tier above pure black to reduce eye strain and OLED artifacts. |
| `--bg-surface` | `255 255 255` (white) | `38 38 38` (neutral-800) | Raised surfaces: cards, inputs, popovers, `.btn-secondary` fill (`bg-surface`). |
| `--bg-subtle` | `245 245 245` (neutral-100) | `64 64 64` (neutral-700) | Subtle hover / zebra fill: neutral & ghost button hover, inline `code`/`pre` (`bg-subtle`). |
| `--bg-muted` | `229 229 229` (neutral-200) | `82 82 91` (zinc-600) | Most prominent neutral fill (`bg-muted`). |
| `--text-primary` | `23 23 23` (neutral-900) | `245 245 245` | Body & heading text (`text-primary`). Also `html/body` colour and the `h1–h6` inherit target. |
| `--text-secondary` | `64 64 64` (neutral-700) | `212 212 212` | Secondary labels, captions; rest colour of `.btn-tertiary` / `.btn-ghost` / `.btn-icon` (`text-secondary`). |
| `--text-muted` | `115 115 115` (neutral-500) | `163 163 163` | Hints, metadata (`text-muted`). |
| `--text-placeholder` | `163 163 163` (neutral-400) | `115 115 115` | Form-field placeholder text (`text-placeholder`). |
| `--border-subtle` | `229 229 229` (neutral-200) | `64 64 64` (neutral-700) | Default hairline; also `borderColor.DEFAULT`, so bare `border` follows the theme. |
| `--border-emphasis` | `212 212 212` (neutral-300) | `115 115 115` (neutral-500) | Stronger borders: inputs, `.btn-secondary`, `.filter-control`. |
| `--accent` | `37 99 235` (blue-600) | `59 130 246` | Primary brand/interactive blue: `.btn-primary` fill, focus rings, links. |
| `--accent-soft` | `239 246 255` (blue-50) | `30 58 138` (blue-900) | Tinted accent wash (`bg-accent-soft`) for soft selected/active states. |
| `--success` | `22 163 74` (green-600) | `34 197 94` | Play / enabled green: `.btn-success`, `.btn-icon-success`. |
| `--success-bg` | `240 253 244` (green-50) | `20 83 45` (green-900) | Soft success surfaces / banners (`bg-success-bg`). |
| `--danger` | `220 38 38` (red-600) | `239 68 68` | Stop / Delete / error red: `.btn-danger`, `.btn-icon-danger` hover. |
| `--danger-bg` | `254 226 226` (red-100) | `127 29 29` (red-900) | Soft danger surfaces / error boxes (`bg-danger-bg`). |
| `--warning-bg` | `255 251 235` (amber-50) | `120 53 15` (amber-900) | Soft warning banners (`bg-warning-bg`). |
| `--warning-text` | `146 64 14` (amber-800) | `252 211 77` | Warning foreground **and** the solid `.btn-warning` fill, the `.bg-highlight` glow source. |

The `colors` vs `textColor` split matters: surface/border/accent/state tokens drive `bg-*` / `border-*` / `ring-*`, while text tokens live in a separate `textColor` map keyed `primary`/`secondary`/`muted`/`placeholder`. That is why `text-muted` resolves to `--text-muted` and `bg-muted` resolves to `--bg-muted` — same bare name, different token. Use `bg-muted` for the surface fill and `text-muted` for muted text; they are not interchangeable.

## 2. Theming

- **Light / dark is class-scoped.** `:root` is light, `.dark` overrides. The dark palette is lifted one tier off pure black (base = neutral-900, surface = neutral-800) — mirrors shadcn/ui 2025 and GitHub Dimmed — because neutral-950 tires the eyes against light text.
- **`color-scheme` is set per theme** (`light` in `:root`, `dark` in `.dark`). This is required, not cosmetic: without it WebKitGTK draws native controls (the language `<select>`, scrollbars, checkboxes) with the system light widget regardless of CSS — a white box on the dark theme.
- **Native chrome is themed in `@layer base`** so component classes still win:
  - `html, body` get `bg-base` + `text-primary` + `user-select: none` (native-app feel).
  - `h1–h6` are forced to `color: inherit` — Tailwind preflight resets heading size/weight to inherit but not colour, so bare dark-mode headings would paint browser-black at first paint.
  - `input, textarea` get `--bg-surface` background, `--text-primary` colour, `--border-emphasis` border, `--text-placeholder` placeholder, and opt back into `user-select: text`.
  - Bare `<button>` is `background-color: transparent; color: inherit` so hand-rolled buttons follow the surface instead of native white. `.btn-*` classes override.
  - `img` is drag-disabled (`-webkit-user-drag: none`) — mod icons, pack covers, world thumbnails are display-only.
  - `*:focus-visible` draws a `2px solid rgb(var(--accent))` outline at `2px` offset (the global keyboard-nav fallback ring).

## 3. Typography

Lucerna uses Tailwind's **default** type scale and system-`sans` font stack — `tailwind.config.cjs` extends only `colors`/`textColor`/`borderColor`, no `fontFamily` or `fontSize` override. The only font-family exception is the Microsoft sign-in button, which hardcodes `Segoe UI` for brand compliance; never copy it elsewhere.

**Practical scale (native-app chrome):**

- **Body / controls:** `text-sm` is the readable default; `text-xs` for dense/meta rows. These two carry the app. Default body copy → `text-sm text-secondary`; captions/metadata → `text-xs text-muted`.
- **Section & modal titles:** `text-base font-semibold text-primary` is the de-facto heading (settings panels, most compact dialogs).
- **Larger dialog headings:** `text-lg font-semibold text-primary` is used by some prominent dialogs today (import, restore, quick-join, delete, backups, export, wizard).
- **Rule for new dialogs:** default the title to `text-base font-semibold`; reserve `text-lg` for full-screen-replacing flows (the import/export wizards). The `text-lg` confirm dialogs above predate this rule (see Known gaps) — don't copy them as precedent.
- **Hero identity only:** `text-xl font-bold` (instance name), `text-2xl font-extrabold` (instance monogram tile), `font-bold text-lg` (sidebar "Lucerna" wordmark). Do not reach for `text-xl+` / `font-extrabold` for ordinary headings.
- **Eyebrow / field labels:** uppercase micro-caps — `text-xs uppercase tracking-wide text-muted` (group eyebrows) and `text-xs uppercase text-secondary mb-1` (form labels); `text-[10px] uppercase` for the tiniest stat captions. These two conventions are not unified — they differ in colour (`text-muted` eyebrow vs `text-secondary` form label) and tracking (`tracking-wide` here, `tracking-wider` elsewhere); pick the closest existing precedent (see Known gaps).
- **Monospace:** `font-mono` (usually `+ text-xs`) for machine values — file paths, mod IDs, MC versions, JVM args, log output, masked keys. Applied ad-hoc (no shared utility).

**Weights are a two-step system:** `font-medium` (button/label default, emphasized inline text) → `font-semibold` (titles, eyebrows). `font-bold`/`font-extrabold` are reserved for brand/instance hero.

**Native-app selection.** `body` sets `user-select: none` so the user cannot lasso-select arbitrary UI chrome. Text opts back in three ways:

- the **`.selectable`** utility — add it to any copyable text (log lines, descriptions, the About disclaimer, diagnosis excerpts, file paths);
- all `input`/`textarea`;
- `[contenteditable="true"]`.

**`.prose-body` (injected description HTML).** `RenderedBody.svelte` injects backend-sanitized remote markdown via `{@html}`, which Svelte scoped styles can't reach, so `.prose-body` globals re-establish prose typography that preflight stripped: its own heading scale (`h1` 1.25rem → `h2` 1.125rem → `h3` 1rem), `text-sm text-secondary leading-relaxed` body, restored list markers, `hr`, blockquote, tables, code/`pre` chips, and **always-underlined** accent links (clicks are intercepted and opened externally so an `<a>` can't navigate the webview away from the SPA). Apply `.prose-body` *only* to rendered remote HTML — never to app-authored markup. Note its links are the inverse of the app's own inline link `.btn-link`, which underlines on *hover*.

## 4. Buttons

Every button reaches for a `.btn-*` purpose class plus a size class — never hand-rolled Tailwind. The shared base (primary/success/danger/secondary) applies `rounded font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed` plus an accent focus-visible ring.

| Variant | Purpose | When to use |
|---|---|---|
| `.btn-primary` | Solid accent CTA | The single most important action in a context (Save, Confirm, Add, Done, Install). |
| `.btn-success` | Solid green | Play / Ready. Distinct from primary so it never competes with it. |
| `.btn-danger` | Solid red | Stop, Delete confirmation, destructive actions. |
| `.btn-secondary` | Outlined neutral (`bg-surface` + emphasis border) | Repeatable, non-headline tools: Manage, Cancel, View, Mods folder. Most "do a thing" buttons land here. |
| `.btn-tertiary` | Text-only neutral, underline on hover | Inline soft actions (Clear filters). Neutral colour. |
| `.btn-ghost` | Borderless neutral, soft hover bg | Low-key action next to a primary CTA (tour "Skip"). Reads as a button, sits below `.btn-secondary`. |
| `.btn-ghost-danger` | Borderless destructive, `bg-danger/10` hover | Left-side soft-delete in a footer (precedes the right-side primary). Uses a one-off **danger** focus outline; `btn-sm` padding baked in. |
| `.btn-warning` | Solid amber (`bg-warning-text` fill, `text-surface` label) | A loud warning CTA that must dominate (e.g. the CTA inside a CurseForge-key banner). |
| `.btn-warning-soft` | Soft amber (`bg-warning-bg` + 30%-opacity amber border) | Full-width info banners where solid would be too loud. Hover deepens the border. |
| `.btn-link` | Accent hyperlink, underline on hover | Inline "open the page" links; pair with a trailing `<Icon name="externalLink" />`. |

**Sizes** layer on top: `.btn-xs` (`px-2 py-1 text-xs`, card/toolbar density) · `.btn-sm` (`px-3 py-1.5 text-sm`, the default — there is no `btn-md`) · `.btn-lg` (`px-4 py-2 text-base font-semibold`, hero CTAs; the only size that bumps weight).

**Icon-only buttons** use the square family with a colour-by-state model (no background plate; hover feedback is colour + zoom):

- `.btn-icon` — 2rem square (= `.btn-sm` height), `text-secondary` at rest, darkening to `text-primary` on hover.
- `.btn-icon-sm` — 1.75rem; pair *with* `.btn-icon` for dense rows.
- `.btn-icon-danger` — neutral at rest, **danger only on hover/focus** (destructive, always rendered).
- `.btn-icon-warning` / `.btn-icon-success` — **static** colour (the colour *is* the state: update-available / enabled); no hover model. These tonal variants are excluded from the neutral hover rule via `:not()` selectors.

**Choosing a button:**

- Most important action here → `.btn-primary` (Play → `.btn-success`).
- Destructive → `.btn-danger`; soft footer delete → `.btn-ghost-danger`.
- Secondary repeatable tool → `.btn-secondary`.
- Inline soft text action → `.btn-tertiary`; external link → `.btn-link`.
- Low-key neighbour of a CTA → `.btn-ghost`.
- Icon-only → `.btn-icon` (+ a tonal modifier if state-coloured).
- Size: `.btn-sm` by default, `.btn-xs` for dense cards/toolbars, `.btn-lg` for the one hero action.

The `Variant`/`Size` unions in `button-matchers.ts` are **closed** — a new variant must be added there. The matchers split the className and assert `btn-<variant>` / `btn-<size>` inclusion; they're used by the `button-intents-*` and `close-button` tests.

## 5. Action affordances (icon vs label · tooltip · animation)

An action's affordance is decided by its **role and density**, not its verb. A repeatable per-row / per-card / toolbar tool whose meaning is carried by a well-known glyph (delete, install, toggle, refresh, move, search-next) renders **icon-only** on `.btn-icon` / `.btn-icon-sm` and MUST carry a `use:tooltip` *and* an `aria-label` from the **same** i18n key. A focal, committing, or standalone action — a modal-footer Cancel/Save, a destructive *confirm*, a multi-select bulk action, a link that opens a page — renders **text-labelled**, no icon (or a trailing `externalLink` for the link role). An icon is **added to a label** (always *leading*) only for the loud launch / create CTAs and refresh tools. Two carve-outs are permanent: the green **Play / Connect** CTA keeps `.btn-success` even when icon-only (its colour *is* its meaning, so it is not demoted to `.btn-icon`), and **Open-folder is always icon+label**, never icon-only.

| Action | Affordance | Tooltip | `aria-label` | Animation |
|---|---|---|---|---|
| Delete / uninstall (dense row/card) | icon-only `.btn-icon-sm .btn-icon-danger` | yes (= aria-label) | yes | zoom |
| Destructive confirm (dialog footer) | text `.btn-danger` (or `.btn-ghost-danger` in bars) | no | — | none |
| Refresh / recheck | text `.btn-secondary` + leading `<Icon name="refresh" class="icon-spin-hover" />` | no | — | spin 180° |
| Apply available update (badge) | icon-only `.btn-icon-sm .btn-icon-warning` | yes | yes | zoom — **no spin** (not a re-run) |
| Enable / disable toggle | icon-only (`.btn-icon-success` on, `!text-muted` off) | yes | yes | zoom; colour = state |
| Install (dense version / card row) | icon-only `.btn-icon-sm !text-accent` | yes | yes | zoom; `Spinner` while busy |
| Install (prominent CTA) | icon+label `.btn-primary` + leading `download` | no | — | none |
| Open-folder (always) | icon+label `.btn-secondary` + leading `folderOpen` — never icon-only | no | — | none |
| Open-external / view on page | `.btn-link` + trailing `externalLink` | only if it carries the URL | — | none |
| Inline "jump elsewhere" | text-only `.btn-tertiary`, no icon | no | — | none |
| Close / dismiss | shared `<CloseButton>` | yes | yes (required prop) | zoom |
| Kebab / overflow (toolbar / header) | shared `<OverflowMenu>` | yes | yes | zoom |
| Expand / collapse `<details>` | `<Icon name="caret" />` in `.disclosure-caret` | — | — | caret rotate 90° |
| Headline launch (Play / Stop / Install) | icon+label `.btn-lg` + leading glyph | no | — | none |
| Play / Connect (icon-only secondary) | icon-only on `.btn-success .btn-sm` — carve-out, *not* `.btn-icon` | yes (on wrapping `<span>`) | yes | none |
| Move / cancel-queue / search prev-next | icon-only `.btn-icon-sm` (cancel → `.btn-icon-danger`) | yes | yes | zoom |
| Disabled-capable icon button | as its type, but `use:tooltip` on a wrapping `<span>` (a disabled button fires no pointer events); `aria-label` stays on the button | yes (span) | yes (button) | zoom |

The canonical reference row is [`ModCard.svelte`](../src/lib/mods/ModCard.svelte) — icon-only enable / update / install / delete, each with tooltip + aria-label; `CloseButton` and `OverflowMenu` are the shared dismiss / overflow primitives.

**Tooltip rule.**

- Every icon-only action button carries **both** `use:tooltip` and `aria-label`, from the same i18n key — the tooltip is the visible mirror of the accessible name, surfaced once not twice. This holds even for "obvious" header singletons (the back arrow, close).
- **Never `title=`** — it is unreliable for assistive tech / touch and bypasses the singleton tooltip layer. (A `title` prop forwarded *into* `use:tooltip` internally is fine — the prop name is incidental.)
- A **disabled-reason** tooltip wraps the control in `<span use:tooltip={{ text, describe: false }}>`; `describe: false` marks it as *supplementary* info, not the accessible name.
- **Text-labelled buttons take no tooltip** (the label is the name) — except a `.btn-link` whose tooltip carries the destination URL.
- **`HelpPopover` (`(?)`) is for a sentence of conceptual help next to a header — never to name an action.** A hover tooltip names a control tersely; a HelpPopover explains.
- Tooltip copy is i18n (`$t`); raw strings only for non-translatable data (URLs, file paths).

**Animation rule.** Motion is a small, mostly-semantic vocabulary; nothing else may be hand-rolled on a button.

- **zoom** (`.fx-icon-zoom`, scale 1.2) — decorative and **uniform**: every `.btn-icon` / `.btn-icon-sm` gets it automatically (root-gated by the `iconZoomFx` pref). Labelled buttons never zoom.
- **spin** (`.icon-spin-hover`, 180° on hover) — means "point at this and it **re-runs**": refresh / recheck only, always with `name="refresh"`; **withheld** from the same-glyph apply-update action (which is not a re-run).
- **caret rotate** (`.disclosure-caret`, 90°) — every `<details>` disclosure. A menu / select trigger rotates a `chevronDown` 180° to mean *open* — use the shared rotation, never an inline one-off.
- **rainbow** (`.icon-rainbow-hover`) — playful delight on the two opt-in sidebar icons only (Browse-modpacks, Shaders); never on a destructive / primary / task action.
- **continuous spin** is **only** `<Spinner>` (= "loading"), distinct from the hover-only refresh spin.
- No ad-hoc `transition-transform` / `hover:scale` / inline `animate-spin` on buttons. The **icon-button no-background-plate** rule (hover feedback = colour-shift + zoom) currently has zero violations — keep it that way.

Conformance to this section is checked by the design audit.

## 6. Form controls

Lucerna deliberately avoids native OS widgets in favour of small, fully-themeable headless Svelte 5 controls. Two idioms recur: WAI-ARIA roving arrow-key navigation, and the `h-8` / `text-sm` / `border-border-emphasis` / `rounded` sizing baseline expressed via `.filter-control`.

- **`Select`** (`src/lib/ui/Select.svelte`) is the custom listbox: a `role=combobox` `<button>` trigger + a `position: fixed` `role=listbox` popover. **Native `<select>` is banned for dropdowns** because WebKitGTK draws the option popup as an OS widget that ignores CSS and stays light in dark theme. The popover is measured from the trigger on each open, clamped to the viewport with an 8px margin (`MARGIN=8`) and capped at `MAX_POPOVER_HEIGHT=240px`, and flips above when short on room — so it escapes the overflow box of the sidebar/modals. Keyboard model is the select-only combobox: focus stays on the trigger via `aria-activedescendant`, with first-letter typeahead (500ms buffer), Home/End/Arrow nav, Escape (stops propagation so an enclosing modal doesn't also close), and Tab-commit. **Use for any single-select dropdown where you don't want all options visible.**
- **`.filter-control`** is the sizing baseline for filter-row controls: `h-8 px-3 text-sm border border-border-emphasis rounded bg-surface`. `.filter-control-select` adds `w-auto` (a Select sizes to its content); `.filter-control-narrow` is `w-28` for the MC-version combobox. Apply to every input *and* Select trigger in a filter row so they vertically align.
- **`SegmentedControl`** — choose-one-of-N with all options visible. `variant="boxed"` is a bordered pill group (active = `.btn-primary`), used by the grid/list `LayoutToggle`; `variant="inline"` is bare clickable text numbers, used by `PageSizePicker`. Activation follows focus (options act immediately). Use over `Select` when the option set is short and worth showing.
- **`TabBar`** (`src/lib/ui/TabBar.svelte`) — the shared underline-tab strip for switching between mounted content panels (detail-modal sections, Browse|Imported / Browse|Installed sub-tabs). Active tab = `border-accent text-primary font-semibold`; inactive = `border-transparent text-muted`. WAI-ARIA tablist, activation-follows-focus. Not for picking a value to submit.
- **`ToggleChip` / `ToggleChipGroup`** — rounded-full pill chips. `ToggleChip` is standalone **multi-select** (`aria-pressed`, e.g. Logs level chips); `ToggleChipGroup` is **single-select** (`role=radiogroup` / `radio`). Both share the `toggleChipClass(active, tone)` helper across six semantic tones (`neutral`/`accent`/`success`/`warning`/`danger`/`muted`); an inactive chip always falls back to de-emphasised neutral so it never competes with the selected one. Use the group for pick-exactly-one with per-option tones/counts; use chips when several can be active.
- **`Pagination` / `PageSizePicker`** — the shared Steam-style footer for every browser. `Pagination` is presentational and 0-based: First · Prev · "N of M" · Next · Last, with an optional right-pinned `end` snippet. Drop `PageSizePicker` (an inline `SegmentedControl` of page-size numbers, bound to `browserPrefs`) into that `end` slot. `PageSizePicker` takes a `prefsKey` — catalog `pageSize` for Browse/Modpacks, `installedPageSize` for the Installed tab.
- **`McVersionCombobox`** — a text-input combobox for substring-filtering the (long) MC release list, with an "Any version" reset row. Reach for it when `Select`'s first-letter typeahead isn't enough.
- **`SourcePicker`** — a labelled `Select` for the *catalogue context switch* (Modrinth / CurseForge / FTB / ATLauncher), **not** a narrowing filter. Because it changes *which catalogue* you're browsing rather than filtering results, it lives in the sub-tab header row, not the filter toolbar; `allowFtb` / `allowAtlauncher` append the extra sources only on the Modpacks tab. A new catalogue source belongs here.

**Choosing a single-choice (one-of-N) control:**

| Use | When |
|---|---|
| `SegmentedControl` | All options are short and worth showing at once (a view toggle, page size). |
| `ToggleChipGroup` | Pill options that carry per-option tones or counts, all kept visible. |
| `Select` | A long list, or you want a compact trigger that hides the options until opened. |

## 7. Icons

Every UI icon flows through one wrapper, `<Icon name="…" />` (`src/lib/ui/icons/Icon.svelte`), which maps a **semantic** name to a Lucide component via the central registry (`src/lib/ui/icons/registry.ts` — the *only* module importing `@lucide/svelte`; swapping an icon or the whole library is a one-line change there).

- **Defaults:** 16px, stroke-width 2, `currentColor`. Because strokes use `currentColor`, an icon inherits whatever Tailwind `text-*` token its parent sets — light/dark theming is automatic. Set colour with a `text-*` class, not a colour prop.
- **Accessibility:** decorative by default (`aria-hidden`). Pass `label` only when the icon is the sole carrier of meaning — it then becomes `role="img"` + `aria-label`. Leave icons paired with visible text unlabeled.
- **Names are intent-based, not glyph-based.** Notable pairs: `caret` (collapsed) vs `chevronDown` (expanded); `update` (static marker) vs `refresh` (spun-on-hover action) — same `RefreshCw` glyph; `package` (modpack) vs `puzzle` (single mod); `play`/`stop` transport pair.

Three decorative motion effects layer on, each preference-gated and reduced-motion-safe:

- **Hover zoom** (`scale(1.2)` / 120ms) — gated globally by a `.fx-icon-zoom` class on the document root, toggled from the `iconZoomFx` preference (default **on**, `localStorage`-persisted). Scoped strictly to `.btn-icon` / `.btn-icon-sm`; labeled buttons are never touched. The base transition is unconditional, so toggling the preference off just stops applying the scale (no jump).
- **Rainbow cycle** (`.icon-rainbow-hover`, 2.5s linear infinite) — opt-in *per icon*; the Sidebar applies it only when `rainbowFx` is enabled (default on). Keyframes list explicit spectrum hue stops because `color` interpolates in sRGB, not by hue.
- **Spin** (`.icon-spin-hover`, 180° / 0.5s) — for refresh/recheck actions. When combined with zoom in an icon button, both transforms merge into one declaration (a single `transform` can't be split across rules).

All three are zeroed for `prefers-reduced-motion` users via the global block; the zoom additionally forces `transform: none` on its end-state. Note these FX preferences are deliberately client-only (not in the Rust settings struct) — no FOUC need.

## 8. Overlays & dialogs

All overlays split into two families.

**Modal family.** `Modal.svelte` is the *only* dialog primitive — it owns the `fixed inset-0 z-50` backdrop (`bg-black/40`), the centered panel (`role="dialog"` + `aria-modal="true"` + `use:trapFocus` + `bg-surface rounded-lg shadow-xl`), and Escape/backdrop close. Caller passes children plus exactly one of `ariaLabelledby` (preferred, id of an in-panel heading) or `ariaLabel`. Do not hand-roll a backdrop/role/focus-trap — wrap `Modal`. Key behaviours:

- **Topmost-only Escape:** a module-level open-stack means a nested confirm closes one layer per keypress. Modals share `z-50` and stack by DOM order, so render a stacked modal *after* the one it covers.
- **Deliberate backdrop dismissal:** close fires only when both `mousedown` and `mouseup` land on the backdrop (not a `click`), so a drag-selection released past the panel edge doesn't discard the selection.
- **Focus trap + restore** via `use:trapFocus` (`src/lib/ui/trap-focus.ts`): initial focus goes to `[data-autofocus]` else first focusable; on unmount, focus returns to the pre-open element.

**Confirm-dialog convention.** Confirm dialogs are thin presentational wrappers over `Modal` (the mutation stays with the caller). The shape: an `ariaLabelledby`-linked `<h3>` title, one or two `text-sm text-secondary` description paragraphs, and a right-aligned footer with a neutral `.btn-secondary.btn-sm` Cancel + a `.btn-danger.btn-sm` confirm whose label *is* the destructive verb. For **filesystem-irreversible** actions (deleting a world's files), add the stronger gate: a text input requiring the literal word `Delete` (not the world's name — players use unicode/emoji names) before the confirm `BusyButton` enables, plus `closeOnBackdrop={!busy}` / `closeOnEscape={!busy}` lockout while the op runs.

**Fixed-popover family.** `OverflowMenu`, `ContextMenu`, `HelpPopover`, and the tooltip layer all use `position: fixed` to escape host `overflow` boxes, measure from the trigger on open, clamp into the viewport (8px margin), and close on captured `scroll` + `resize`.

- **Menus** (`OverflowMenu` left-click ⋯, `ContextMenu` right-click) share one item model, `ContextMenuItem` (`label`, `icon?`, `danger?`, `disabled?`, `separatorBefore?`, `onSelect`), and identical `role="menu"` / `role="menuitem"` markup with arrow-key roving `activeIndex`, Escape close, and focus return.
- **`CloseButton`** is the shared `×` (a `.btn-icon` rendering the `close` Icon, `aria-label` defaulting to `common.close`). Use it for popovers/headers needing an explicit dismiss; plain `Modal` confirms close via the footer Cancel instead.
- **Tooltips** are a singleton: one `role="tooltip"` bubble mounted in `+layout.svelte`, driven by shared state, opted into via `use:tooltip={label}`. At most one is visible at a time. Never mount per-component tooltip bubbles.
- **`HelpPopover`** is a click-toggle `(?)` helper with persistent body text — distinct from the hover tooltip.

## 9. Cards & status

Cards share a 3-primitive core plus a single status map.

- **`CardShell`** is the outer container for every card surface. It takes `variant` (`'tile'` | `'row'` | `'compact-row'`), `accent`, `dim`, `highlighted`, and a single `children` snippet — there are **no** named media/body/actions slots, so the caller composes the inner row by hand. The shell paints the box chrome, the absolutely-positioned left accent strip, and dim/highlight. Use it for any new card/list-row so accent strip, density, dim, and highlight stay consistent.
- **`CardMedia`** is the leading icon: the project's `iconUrl` as an `<img>`, else a content-kind placeholder glyph. Size is a named token — `sm` (24px) for compact/list rows, `md` (32px) for tiles/list rows, `lg` (40px) for the larger modpack tile. It is strictly a square icon box; there is no banner/cover handling.
- **`StatusBadge`** is the single status pill, with six semantic variants:
  - `success` → up-to-date / positive state
  - `danger` → incompatible / missing-deps
  - `warning` → update-available / distribution-disabled / modified
  - `info` → from-pack / accent-soft chips
  - `neutral` → quiet metadata (downloads, cross-platform) — `text-secondary`
  - `muted` → disabled — `text-muted`

**Status is centralized.** A surface derives a semantic `CardStatusKind` from its own state and asks `cardStatusStyle(kind)` (`src/lib/ui/cards/card-status.ts`) for `{accent, badge, dim}`. Route every card's status through this map rather than picking accent/badge inline — that is the drift-prevention layer. Colour encodes attention, not decoration: `enabled` resolves to **no accent** + a `success` badge, so a screen of installed mods is not a wall of green; only `warning`/`danger`/`info`/`update`/`pack-update` paint the strip. `accentStripClass(accent)` gives the row's left strip; `accentDotClass(accent)` gives the grid-tile corner dot.

## 10. Banners & inline messaging

Severity is expressed entirely through the token families from §1 — never ad-hoc hex. `warning` → `--warning-text` / `--warning-bg`; `danger` → `--danger` / `--danger-bg`; `success` → `--success` / `--success-bg`; `accent` → `--accent` (used for an in-progress / "queued" state). Opacity suffixes (`/30`, `/40`, `/80`) soften borders and secondary hints.

**Soft vs solid:**

- **Soft** (light bg + dark amber text + thin border) is for passive info/diagnosis surfaces and nested confirmation cards. `.btn-warning-soft` is the dedicated soft-banner button.
- **Solid** `.btn-warning` (filled amber, `text-surface` label) is for the one CTA that must dominate inside a banner.
- Within one banner: container is soft, the primary CTA may be solid.

**Diagnosis banner anatomy.** The canonical top-level diagnosis banner (`LogDiagnosisBanner`) is `rounded-xl border border-warning-text bg-warning-bg p-3`, laid out `flex items-start gap-2`: a leading `<Icon name="warning" class="mt-0.5 text-warning-text" />`, then a `flex-1` column with a bold `text-warning-text` title, a `text-sm` body, a "what to try" line, and the action (a `BusyButton.btn-primary.btn-sm`, rendered only when the diagnosis is actionable).

**Nested repair cards** (rendered *inside* an already-amber banner) drop to a quieter container — `mt-3 rounded border border-warning-text/40 bg-surface p-3` — neutral surface bg + faint amber border, so they don't double up the parent's amber wash. Footer: `.btn-primary.btn-sm` confirm + `.btn-secondary.btn-sm` Cancel.

**Danger** has no banner primitive; inline form errors use a red soft box: `bg-danger-bg border border-danger text-danger text-sm rounded p-2`. Note `.bg-highlight` (an 18% amber `color-mix` wash) is *not* a banner — it is the DepTree hover/cross-highlight.

**Inline status words.** A single-word state label reuses the §1 token families *on text*: `set` → `text-success`, `invalid` → `text-danger`, `unverified` → `text-warning-text`, `missing`/quiet → `text-secondary`, `checking` → `text-placeholder` + a `Spinner`. The split between `unverified` (amber — reachability failure, validity unknown) and `invalid` (red — a genuine rejection) is deliberate: don't collapse the two.

**Action hierarchy inside banners & cards.** Confirm = `.btn-primary.btn-sm`, Cancel = `.btn-secondary.btn-sm`, and the single destructive per-row action = solid `.btn-warning.btn-xs`; async actions wrap in `BusyButton` (or render an inline `Spinner`).

**Dismissing banners.** Standing diagnosis/attention banners are closeable by one pattern: a top-right `btn-icon` × (`aria-label` `common.dismissWarning`, tooltip `common.dismissWarningTooltip` — "I know what I'm doing") plus a *restore* badge near the surface's status/header (`common.restoreWarning`), shown only while the banner would otherwise be visible. The restore badge is the shared [`DiagnosisRestoreButton`](../src/lib/ui/DiagnosisRestoreButton.svelte) — a small amber warning-triangle pill (`rounded-full border border-warning-text bg-warning-bg`) in the same amber-pill language as the overview attention-restore badge in [`InstanceHeader`](../src/lib/overview/InstanceHeader.svelte), so the "bring the hidden warning back" control reads the same everywhere. Dismissal is keyed by the diagnosis **signature** and persisted in `localStorage` via [`diagnosis-dismiss.svelte.ts`](../src/lib/ui/diagnosis-dismiss.svelte.ts) — the same webview-preference idiom as the overview attention panel's [`attention-collapse.svelte.ts`](../src/lib/overview/attention-collapse.svelte.ts) — so acknowledging one problem hides exactly that problem while a new/different diagnosis resurfaces on its own. The server crash banner ([`ServerDiagnosisBanner`](../src/lib/servers/ServerDiagnosisBanner.svelte) + restore in `ServerManageView`) and the log diagnosis banner ([`LogDiagnosisBanner`](../src/lib/logs/LogDiagnosisBanner.svelte) + restore in `LogsPopover`) both follow it. **Exception:** a *gate* banner that replaces the surface it guards (e.g. the CurseForge-key banner, which stands in for the whole search UI) is **not** dismissible — there is nothing behind it to reveal, so it carries no ×.

## 11. Loading & busy states

The cross-cutting rule: every loading place renders a spinner **and** a label (visible or screen-reader), and the wrapper carries exactly one `role="status"` + `aria-label` so assistive tech announces the state once.

- **`Spinner`** is the atom (`src/lib/ui/Spinner.svelte`). The arc inherits `currentColor`, so place it inside a `text-*` context to colour it. `size` is `sm`/`md`/`lg`; `labelPlacement` is `sr-only` (default), `right` (inline/buttons, with an explicit label like "Resolving deps"), or `below`. Use the bare `Spinner` for inline / per-row loads.
- **`LoadingPanel`** is the full content-area / empty-dialog-body loader: a centered spinner with the label below, requiring a `label`. Use it for panels and dialog bodies that are otherwise empty while content loads.
- **`BusyButton`** is the controlled async-action button: the parent owns `busy` and sets it around the IPC call (before the first `await`, cleared in `finally`). While busy the button is disabled and `aria-busy`; a small `Spinner` renders **alongside** the children — the text label stays visible. Extra attributes (`data-testid`, `data-tour`, `aria-label`) pass through via `...rest`. Use it for any button firing an async action that should block re-clicks. To attach a tooltip, wrap it in a `<span>` (`use:tooltip` is DOM-element-only).
- **`PhaseStatusRow`** is the bottom-strip install / mod-install progress display: it subscribes to the `installProgress` / `modInstallProgress` events, renders a `Spinner` + phase label + `files_done/total` counter + an accent progress bar, and clears itself on process/mod lifecycle events (spawned / exited / installed / failed). The spinner drops once the phase is `complete`. It is event-driven and surface-specific — *not* a reusable progress primitive (see Known gaps).

**The `delayMs` gotcha.** `Spinner`/`LoadingPanel` support an anti-flicker `delayMs` — the spinner isn't rendered until the load exceeds the delay. `LoadingPanel`'s default is **150ms**, which means a synchronous test doing `getByRole('status')` immediately after render finds nothing. Surfaces whose intent tests assert the loading status synchronously pass `delayMs={0}` (e.g. `ModBrowseView`, `ModpackBrowseView`, `ModpackDetailModal`); others keep the 150ms default and use `waitFor()`. Set `delayMs={150}` for loads that are usually fast and would otherwise flash a spinner.

## 12. Motion

Two hard rules govern all motion: animate only `transform` / `opacity` / `color`, and let the global `prefers-reduced-motion` block zero everything. The vocabulary on top is deliberately small.

- **Reduced-motion policy.** A single global `@media (prefers-reduced-motion: reduce)` block on `*, *::before, *::after` forces `animation-duration` / `transition-duration` to `0.01ms !important`, `animation-iteration-count: 1`, and `scroll-behavior: auto`. The duration is near-zero (not `0`) on purpose, so `transitionend` / `animationend` listeners still fire and nothing waiting on them stalls. New animations need no extra reduced-motion handling.
- **Compositor-friendly only.** Every animated property is `transform`, `opacity`, or `color` — never layout-bound props (`width`/`height`/`top`/`left`/`margin`/`padding`/`border`/`font-size`). Colour animation works because icons use `currentColor`.
- **The motion vocabulary:** icon hover zoom (`scale(1.2)` / 120ms ease-out, preference-gated), rainbow cycle (2.5s linear infinite, opt-in), refresh spin (180° / 0.5s ease), disclosure caret rotate (90° / 0.15s ease), and Tailwind `transition-colors` (~150ms) on button hover/focus. `.btn-tertiary` intentionally omits `transition-colors` (instant underline). `will-change` is not used (acceptable at this scale).

## 13. Accessibility

A11y is convention-driven and centralized in shared primitives:

- **Focus rings.** Global `*:focus-visible` draws a `2px` accent outline at `2px` offset; `.btn-*` classes mirror it (destructive buttons use a danger outline). There is no dedicated focus-ring token — focus reuses `--accent`, so it can't be retuned independently of the primary accent, and the app uses `outline-accent`, not Tailwind `ring-*` utilities (which are unthemed). Known debt (**B2**): hand-rolled controls still rely on the global fallback ring rather than the crisper button rings.
- **Keyboard-operable custom controls.** Any non-`<button>` element given `role="button"` + `tabindex` must wire an `Enter`/`Space` `keydown` handler so it works without a mouse — `FileDropzone` is the reference implementation. Known debt: a few `role="button"` rows (e.g. in `WorldsTab`) still rely on click only.
- **Modal a11y.** Dialogs render through `Modal` (`role="dialog"`, `aria-modal`, `tabindex="-1"`, `aria-labelledby`/`aria-label`, focus trap + restore, topmost-only Escape stack).
- **WAI-ARIA roles for composite widgets:** tabs use `tablist`/`tab` with roving tabindex + arrow keys; the custom `Select` uses `combobox`/`listbox`/`option` with `aria-activedescendant`; menus use `menu`/`menuitem`; chip groups use `radiogroup`/`radio`.
- **Live regions — role, not colour, signals urgency.** Pending state uses `role="status"`; errors that render *after* mount use `role="alert"`; progress uses `aria-live="polite"`. The toast region is an always-mounted `aria-live="polite"` container.
- **Reduced motion** is honoured via the global CSS block (plus Tailwind `motion-reduce:` and a JS `matchMedia` check in the tooltip layer). Per-theme `color-scheme` keeps native controls legible in dark. Icons are `aria-hidden` unless given a `label`.

## 14. Layout

Lucerna is a single SvelteKit route (`src/routes/+page.svelte` holds the whole app — sidebar, tabs, all modals); navigation is state-driven, not client routing. The thin `+layout.svelte` imports `app.css`, mounts the singleton `TooltipLayer`, blocks the native WebView context menu, and toggles the root `.fx-icon-zoom` class.

- **App shell grid.** `<main class="grid h-screen overflow-hidden">` with columns set reactively from compact state — expanded is `240px 1fr` (sidebar + content), compact is a single `1fr` column — and rows `1fr auto` (content row + a bottom status/phase strip). When expanded the sidebar spans both rows (`1 / -1`) so the status strip sits only under the content column; the right content column unmounts entirely in compact mode.
- **Tab navigation.** Two patterns: `MainTabs` hand-rolls the per-instance Overview / Add-ons / Worlds tablist inline, while `TabBar` is the reusable underline-tab primitive for modals and sub-tabs. Both use the same WAI-ARIA roving-tabindex + activation-follows-focus model and the same active recipe (`border-accent text-primary font-semibold`). They drift in chrome, though: `MainTabs` tabs are `text-base` on a `px-3 bg-surface` strip, while `TabBar` is `text-sm` on a bare strip — a future tab-style change must touch both (see Known gaps).
- **Sidebar rhythm.** `<aside data-sidebar … p-3>` wraps a `flex flex-col gap-3` column. Sections are `flex flex-col gap-1` blocks separated by `pt-3 border-t border-border-subtle`, each prefixed with a `text-xs uppercase tracking-wide text-muted` heading. The `data-sidebar` / `data-sidebar-content` hooks are required — the compact-mode `ResizeObserver` measures content height to size the OS window.
- **Compact/expanded** is a `$state` rune (`compact.svelte.ts`) that reshapes the grid and resizes the window; opening any wide overlay (Manage / Modpacks / Logs / Settings / Export / MS sign-in) auto-expands.
- **Sidebar attention badges.** A nav button signals state through its **leading icon's colour** plus an optional **inline wrench** after the label — not a corner dot. Servers resolves `fixable > crashed > running > idle` (via `serversNavStatus`): `fixable` (a crash with a one-click fix) → amber icon (`text-warning-text`) + `<NavFixWrench>`; `crashed` (a crash, no fix) → red icon (`text-danger`); `running` → green icon breathing **saturation** via `.nav-icon-running` (`--success` ↔ `--success-muted`, same hue/lightness; `motion-reduce` rests at `--success`); `idle` → neutral. Logs resolves `actionable` (amber icon + wrench) `> advisory` (amber icon) `> idle`. The shared kernel is [`nav-status.ts`](../src/lib/layout/nav-status.ts) (`navVisual`), `NavStatusIcon.svelte`, and `NavFixWrench.svelte`; status is never colour-only — the coloured icon carries `role="img"` + `aria-label` + tooltip. Modpacks uses a count pill instead.

## 15. How these rules are enforced

Two complementary test layers, documented in [`docs/UI-TESTING.md`](UI-TESTING.md):

- **Intent regressions** — cheap vitest class-string assertions via the `button-matchers` custom matchers (`toHaveBtnVariant` / `toHaveBtnSize`), run in `pnpm test`. When you add a prominent button, add a one-line assertion in the relevant `tests/button-intents-*.test.ts`. This is what stops the Install button silently dropping from `.btn-primary.btn-lg`.
- **Render regressions** — Playwright visual snapshots under `tests-e2e/visual/`, run via `pnpm test:e2e` (Linux-pinned; the CI visual job is currently gated off pending committed baselines).

## Known gaps & deviations

This doc is honest about the inconsistencies the codebase carries today:

- **Dialog title size is inconsistent.** Some dialogs use `text-base font-semibold`, others `text-lg font-semibold`. §3 now sets a forward rule (new dialogs default to `text-base`; `text-lg` only for wizard/import flows), but the existing dialogs are not yet reconciled to it — and there's no shared heading/title component, so the pattern is re-declared inline 15+ times with varying class order.
- **Choose-one-of-N is solved three ways** (`Select`, `SegmentedControl`, `ToggleChipGroup`) with overlapping use cases; the "when to use which" guidance lived only in scattered file headers before this doc.
- **`.btn-warning-soft` is under-adopted.** It was purpose-built for full-width info banners, yet the CurseForge-key banner hand-rolls the identical token recipe inline (and uses a *solid* `.btn-warning` CTA). Banner borders also drift across three opacities (full / `/40` / `/30`) and two radii (`rounded-xl` vs `rounded`).
- **Two `ModpackCard` components share a filename** with different APIs — `modpacks/ModpackCard` (search grid, which does *not* use `CardShell`) and `overview/ModpackCard` (a bespoke Overview-tab section card). Container metrics (radius/padding) drift between card surfaces because they aren't tokenized.
- **Duplicated primitives.** Roving keyboard nav is copy-pasted across `TabBar` / `SegmentedControl` / `ToggleChipGroup`; the fixed-popover close-on-scroll/resize logic is reimplemented in three overlays; `OverflowMenu` and `ContextMenu` render byte-identical menu markup; there is no shared `ConfirmDialog` (confirm dialogs are near-identical copies); `McVersionCombobox` duplicates `Select`'s popover logic but *without* the viewport-clamp fix, lacks Home/End/typeahead, and maps `aria-selected` to the keyboard-active row rather than the committed value (incorrect ARIA — a latent a11y bug `Select` doesn't have).
- **Ad-hoc literals & z-index.** `font-mono text-xs`, `text-[10px]`, and `text-[0.9em]` bypass the scale tokens; uppercase label micro-caps use competing colour/tracking conventions (`text-muted` + `tracking-wide` vs `text-secondary`, and `tracking-wide` vs `tracking-wider`); popover z-index values are inconsistent (`Select` listbox `z-50`, `McVersionCombobox` `z-30`) with no layering token; durations/easings are inline literals with no `--duration-*` / `--ease-*` tokens.
- **`CardShell` enforces nothing internally.** Its single-`children`-snippet composition (no media/body/actions slots) means inner-row consistency across consumers is convention-only. Grid-tile status signalling also diverges: mods-grid tiles get an `accentDotClass` corner dot, but modpack-grid tiles get none.
- **Decorative icon FX are gated two different ways.** Hover-zoom is switched by one root `.fx-icon-zoom` class; rainbow is opted in per-icon by callers adding `.icon-rainbow-hover`. Same intent, two mechanisms, backed by near-duplicate persisted-boolean fx stores (`iconZoomFx` / `rainbowFx`) — not unified.
- **`HelpPopover` is less keyboard-accessible than `Modal`.** It has no focus trap, doesn't auto-focus its `CloseButton`, and has no Escape-to-close (only click-outside / scroll / resize dismiss it).
- **`Modal` has no `aria-describedby`**, so screen readers announce a confirm dialog's title but not its irreversibility warning.
- **A few one-offs:** `--danger-bg` light is red-100 (every other `*-bg` is the `-50` tier); `.btn-warning`'s `hover:bg-warning-text` is a no-op (no visible hover feedback); solid buttons hardcode `text-white` while `.btn-warning` uses `text-surface`; `role="alert"` is used liberally on persistent server error paragraphs (risking redundant assertive announcements).

// The parsed shape of CHANGELOG.md (Keep a Changelog format). Body text of
// entries stays verbatim English; only section headings are localized, and
// that mapping lives in the panel, not here.

export type SectionKind =
  | 'added'
  | 'changed'
  | 'fixed'
  | 'deprecated'
  | 'removed'
  | 'security'
  | 'other';

export interface ChangelogSection {
  /** Mapped Keep-a-Changelog kind; unrecognized headings map to 'other'. */
  kind: SectionKind;
  /** The heading text exactly as written, e.g. "Added". */
  heading: string;
  /** One entry per bullet; verbatim body text. */
  items: string[];
}

export interface ChangelogVersion {
  /** The bracketed token, e.g. "0.11.0" or "Unreleased". */
  version: string;
  /** Release date as written, e.g. "2026-06-05", or null when absent. */
  date: string | null;
  /** Resolved from the trailing link-reference block, or null. */
  url: string | null;
  sections: ChangelogSection[];
}

/** Versions in file order (newest first). */
export type Changelog = ChangelogVersion[];

// ---------------------------------------------------------------------------
// Display model: the English structure with the active locale's text laid over
// it (see localize.ts). The panel renders this, never `Changelog` directly.

/** One bullet as shown: the locale's text when it has one, else the source. */
export interface DisplayItem {
  text: string;
  /** True when `text` is in the active locale (always true for the source locale). */
  localized: boolean;
}

export interface DisplaySection {
  /** Always the source kind — a translated heading never decides the label. */
  kind: SectionKind;
  /** The locale's heading when aligned, else the source one; rendered only for 'other'. */
  heading: string;
  items: DisplayItem[];
}

export interface DisplayVersion {
  version: string;
  date: string | null;
  url: string | null;
  sections: DisplaySection[];
  /** 'full' = every bullet localized (or nothing to localize); 'partial' = some
   *  fell back to the source; 'none' = the whole version is source text. */
  coverage: 'full' | 'partial' | 'none';
}

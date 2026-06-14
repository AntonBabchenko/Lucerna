// Single source of truth for how a card communicates status. Surfaces derive a
// semantic `CardStatusKind` from their own state and ask this module for the
// matching accent tone, badge variant, and whether the row is dimmed. Keeping
// the mapping here is what stops the per-surface inline-class drift the redesign
// is fixing. Color encodes attention, not decoration: "enabled/normal" stays
// quiet; warning/danger/info stand out.

export type CardAccent = 'none' | 'success' | 'muted' | 'warning' | 'info' | 'danger';
export type BadgeVariant = 'success' | 'muted' | 'warning' | 'info' | 'neutral' | 'danger';

export type CardStatusKind =
  | 'none' // browse, not installed
  | 'enabled'
  | 'disabled'
  | 'update'
  | 'from-pack'
  | 'cross-platform'
  | 'incompatible'
  | 'missing-deps'
  | 'distribution-disabled'
  | 'modified'
  | 'pack-update';

export interface CardStatusStyle {
  accent: CardAccent;
  badge: BadgeVariant;
  dim: boolean;
}

const STYLE: Record<CardStatusKind, CardStatusStyle> = {
  none: { accent: 'none', badge: 'neutral', dim: false },
  // "installed & fine" stays quiet (accent none) — a screen full of enabled mods
  // must not be a wall of green. The success *badge* still carries the state.
  enabled: { accent: 'none', badge: 'success', dim: false },
  disabled: { accent: 'muted', badge: 'muted', dim: true },
  update: { accent: 'warning', badge: 'warning', dim: false },
  'from-pack': { accent: 'info', badge: 'info', dim: false },
  'cross-platform': { accent: 'none', badge: 'neutral', dim: false },
  incompatible: { accent: 'danger', badge: 'danger', dim: false },
  'missing-deps': { accent: 'danger', badge: 'danger', dim: false },
  'distribution-disabled': { accent: 'warning', badge: 'warning', dim: false },
  modified: { accent: 'warning', badge: 'warning', dim: false },
  'pack-update': { accent: 'success', badge: 'success', dim: false },
};

export function cardStatusStyle(kind: CardStatusKind): CardStatusStyle {
  return STYLE[kind];
}

// Left accent strip background utility for a tone. `none` is transparent so the
// strip slot keeps consistent geometry across rows without painting color.
export function accentStripClass(accent: CardAccent): string {
  switch (accent) {
    case 'success':
      return 'bg-success';
    case 'muted':
      return 'bg-border-emphasis';
    case 'warning':
      return 'bg-warning-text';
    case 'info':
      return 'bg-accent';
    case 'danger':
      return 'bg-danger';
    default:
      return 'bg-transparent';
  }
}

// Grid-tile corner status dot — the tile-form expression of the accent strip
// (a rounded tile reads a flush left strip poorly). Same tone mapping.
export function accentDotClass(accent: CardAccent): string {
  return accentStripClass(accent);
}

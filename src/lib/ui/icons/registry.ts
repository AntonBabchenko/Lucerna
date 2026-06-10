import {
  ArrowRight,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ChevronUp,
  Download,
  EllipsisVertical,
  ExternalLink,
  Info,
  LayoutGrid,
  List,
  Minus,
  Package,
  Plus,
  Puzzle,
  RefreshCw,
  TriangleAlert,
  X,
} from '@lucide/svelte';
import type { Component } from 'svelte';

// Semantic name → Lucide component. This is the ONLY module that imports
// from @lucide/svelte; every consumer goes through <Icon name="…" />, so
// swapping an icon or the library is a one-line change here.
export const ICONS = {
  // Disclosure caret. Two intentional reveal mechanisms: native <details>
  // rotate it 90° via the .disclosure-caret CSS rule; manual toggles instead
  // swap the name between `caret` (collapsed) and `chevronDown` (expanded).
  caret: ChevronRight,
  chevronDown: ChevronDown,
  chevronUp: ChevronUp,
  close: X,
  warning: TriangleAlert,
  info: Info, // informational marker (e.g. skipped-override notices)
  success: Check,
  download: Download,
  update: RefreshCw, // static "update available" marker
  refresh: RefreshCw, // action variant — same icon as `update`, but spun via CSS on hover
  plus: Plus,
  minus: Minus,
  list: List,
  grid: LayoutGrid,
  // Second pass — non-indicator icons.
  package: Package, // modpack placeholder avatars + "from modpack" chips
  externalLink: ExternalLink, // opens an external site (e.g. OptiFine hint)
  moreVertical: EllipsisVertical, // overflow-menu affordance (lucide renamed MoreVertical → EllipsisVertical)
  chevronLeft: ChevronLeft, // gallery prev
  chevronRight: ChevronRight, // gallery next
  puzzle: Puzzle, // single-mod placeholder avatar (distinct from package = modpack)
  arrowRight: ArrowRight, // version-transition marker (v1 → v2)
} satisfies Record<string, Component>;

export type IconName = keyof typeof ICONS;

export const ICON_NAMES = Object.keys(ICONS) as IconName[];

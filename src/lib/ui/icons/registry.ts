import {
  Aperture,
  Archive,
  ArrowLeft,
  ArrowRight,
  ArrowRightLeft,
  ArrowUp,
  ArrowUpRight,
  Ban,
  Blocks,
  Check,
  ChevronDown,
  ChevronFirst,
  ChevronLast,
  ChevronLeft,
  ChevronRight,
  ChevronUp,
  CircleX,
  Copy,
  Crop,
  Download,
  EllipsisVertical,
  Eraser,
  Expand,
  ExternalLink,
  Eye,
  EyeOff,
  FolderOpen,
  Globe,
  Hand,
  Highlighter,
  Image,
  Images,
  Info,
  LayoutGrid,
  List,
  Lock,
  Minus,
  Package,
  Pencil,
  Play,
  Plus,
  Power,
  Puzzle,
  RefreshCw,
  RotateCcw,
  ScrollText,
  Server,
  Settings,
  Shirt,
  Shrink,
  SlidersHorizontal,
  Square,
  Trash2,
  TriangleAlert,
  Undo2,
  Upload,
  User,
  UserPlus,
  Wrench,
  X,
  ZoomIn,
  ZoomOut,
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
  upload: Upload, // share/upload action (Logs → Share to mclo.gs)
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
  chevronLeft: ChevronLeft, // gallery prev / pagination prev
  chevronRight: ChevronRight, // gallery next / pagination next
  chevronFirst: ChevronFirst, // pagination first
  chevronLast: ChevronLast, // pagination last
  puzzle: Puzzle, // single-mod placeholder avatar (distinct from package = modpack)
  arrowLeft: ArrowLeft, // back / prev navigation (tour)
  arrowRight: ArrowRight, // version-transition marker (v1 → v2) / next / CTA
  arrowUpRight: ArrowUpRight, // jump-to-row (dep tree internal navigation)
  // i18n tier — nav/action + status.
  settings: Settings, // ⚙ Settings (gear) — app settings entry point
  sliders: SlidersHorizontal, // 🎚 Manage instances (tune/adjust a profile)
  folderOpen: FolderOpen, // 📁 Open folder (instance dir, mods folder)
  trash: Trash2, // 🗑 Delete
  eraser: Eraser, // ✏ Clear / wipe log content
  blocks: Blocks, // 📂 Mods content kind (Add-ons tab kind switch)
  scrollText: ScrollText, // 📜 Logs
  arrowUp: ArrowUp, // ↑ Updates filter
  circleX: CircleX, // ✕ missing status
  play: Play, // Quick Play action — launch into a specific world / server
  stop: Square, // ■ Stop the running game (transport pair with play ▶)
  power: Power, // enable/disable toggle (mods only; RP/shaders have no toggle)
  user: User, // author marker in browse metadata
  userPlus: UserPlus, // add an offline account
  shirt: Shirt, // skin & cape cosmetics entry point
  shrink: Shrink, // collapse window to mini mode
  expand: Expand, // restore window from mini mode
  globe: Globe, // join-server satellite (Quick Play)
  // Content kinds (Add-ons tab): picker options + per-kind placeholder avatars.
  resourcePack: Image, // resource-pack kind
  shader: Aperture, // shader kind
  server: Server, // own-server entry point (sidebar + server list rows)
  switch: ArrowRightLeft, // version row: "switch to this installed version"
  lock: Lock, // restricted / distribution-blocked version (download disabled)
  archive: Archive, // create a backup ("Back up now")
  restore: RotateCcw, // restore a backup
  wrench: Wrench, // a one-click repair is available for a log issue (Logs badge + attention panel)
  eye: Eye, // reveal password / show secret
  eyeOff: EyeOff, // hide password / conceal secret
  edit: Pencil, // edit-in-place affordance (instance avatar hover overlay)
  copy: Copy, // copy image to clipboard (screenshots lightbox)
  gallery: Images, // screenshots gallery (sidebar entry + gallery header)
  clear: Ban, // clear all annotations (distinct from trash = delete file)
  crop: Crop, // crop tool (screenshot annotator)
  hand: Hand, // pan tool (screenshot annotator)
  marker: Highlighter, // freehand draw tool (screenshot annotator)
  undo: Undo2, // undo last stroke (screenshot annotator)
  zoomIn: ZoomIn, // zoom in (screenshot annotator)
  zoomOut: ZoomOut, // zoom out (screenshot annotator)
} satisfies Record<string, Component>;

export type IconName = keyof typeof ICONS;

export const ICON_NAMES = Object.keys(ICONS) as IconName[];

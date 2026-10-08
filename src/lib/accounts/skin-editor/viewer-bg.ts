// The skin editor's viewport backgrounds: a neutral grey like a 3ds Max viewport, and three
// Minecraft places drawn as panoramas by backdrops.ts. One table for the viewport
// (SkinEditorModal), the picker (SkinEditorFooter) and the remembered choice.
import type { BackdropKind } from '$lib/accounts/skin-editor/backdrops';
import type { TranslationKey } from '$lib/i18n/keys.generated';

export const VIEWER_BGS = ['neutral', 'plains', 'nether', 'stronghold'] as const;
export type ViewerBg = (typeof VIEWER_BGS)[number];

export const isPanorama = (bg: ViewerBg): bg is BackdropKind & ViewerBg => bg !== 'neutral';

const NEUTRAL = 'linear-gradient(#8c8c8c, #404040)';

/** CSS background behind the transparent 3D canvas: the neutral gradient, or a dark stand-in
 *  shown while a panorama is being drawn. */
export const VIEWPORT_CSS: Record<ViewerBg, string> = {
  neutral: NEUTRAL,
  plains: '#1c1c1f',
  nether: '#1c1c1f',
  stronghold: '#1c1c1f',
};

/** The picker's swatch for each background: a few bands of the place's own colours. */
export const SWATCH_CSS: Record<ViewerBg, string> = {
  neutral: NEUTRAL,
  plains: 'linear-gradient(#78a7ff 0 50%, #a8c4a0 50% 60%, #7cb64e 60%)',
  nether: 'linear-gradient(#5a1612 0 55%, #f08a24 55% 70%, #6e2a26 70%)',
  stronghold: 'repeating-linear-gradient(#7b7b7d 0 3px, #4f4f51 3px 4px)',
};

export const BG_LABEL: Record<ViewerBg, TranslationKey> = {
  neutral: 'skinEditor.bgNeutral',
  plains: 'skinEditor.bgPlains',
  nether: 'skinEditor.bgNether',
  stronghold: 'skinEditor.bgStronghold',
};

const KEY = 'lucerna.skinEditorBackground';

const isViewerBg = (v: unknown): v is ViewerBg =>
  typeof v === 'string' && (VIEWER_BGS as readonly string[]).includes(v);

/** The background chosen last time; neutral when there is none or it cannot be read. */
export function loadViewerBg(storage: Storage | undefined = globalThis.localStorage): ViewerBg {
  try {
    const v = storage?.getItem(KEY);
    return isViewerBg(v) ? v : 'neutral';
  } catch {
    // Storage can refuse to be read (a denied origin, a broken profile). The choice is a
    // convenience, so the editor opens on the default instead of failing.
    return 'neutral';
  }
}

export function saveViewerBg(
  bg: ViewerBg,
  storage: Storage | undefined = globalThis.localStorage,
): void {
  try {
    storage?.setItem(KEY, bg);
  } catch {
    // A refused write (quota, a denied origin) only means the choice is not remembered next
    // time; it still applies for this session, so there is nothing to tell the user.
  }
}

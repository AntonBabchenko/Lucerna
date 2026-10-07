// The skin editor's three viewport backgrounds. One table for the viewport
// (SkinEditorModal) and the background picker (SkinEditorFooter), so a swatch
// always shows the colour it switches to. Arbitrary Tailwind classes compile
// from .ts too: tailwind.config.cjs scans src/**/*.{svelte,ts,js}.
import type { TranslationKey } from '$lib/i18n/keys.generated';

export const VIEWER_BGS = ['dark', 'mid', 'light'] as const;
export type ViewerBg = (typeof VIEWER_BGS)[number];

export const BG_CLASS: Record<ViewerBg, string> = {
  dark: 'bg-[#1c1c1f]',
  mid: 'bg-[#4a4a50]',
  light: 'bg-[#c9c9cf]',
};

export const BG_LABEL: Record<ViewerBg, TranslationKey> = {
  dark: 'skinEditor.bgDark',
  mid: 'skinEditor.bgMid',
  light: 'skinEditor.bgLight',
};

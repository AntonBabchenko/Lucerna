import type { Component } from 'svelte';
import {
	Check,
	ChevronDown,
	ChevronRight,
	ChevronUp,
	Download,
	LayoutGrid,
	List,
	Minus,
	Plus,
	RefreshCw,
	TriangleAlert,
	X,
} from '@lucide/svelte';

// Semantic name → Lucide component. This is the ONLY module that imports
// from @lucide/svelte; every consumer goes through <Icon name="…" />, so
// swapping an icon or the library is a one-line change here.
export const ICONS = {
	caret: ChevronRight, // disclosure caret; rotated via CSS on details[open]
	chevronDown: ChevronDown,
	chevronUp: ChevronUp,
	close: X,
	warning: TriangleAlert,
	success: Check,
	download: Download,
	update: RefreshCw, // static "update available" marker
	refresh: RefreshCw, // refresh/recheck action buttons (CSS spin on hover)
	plus: Plus,
	minus: Minus,
	list: List,
	grid: LayoutGrid,
} satisfies Record<string, Component>;

export type IconName = keyof typeof ICONS;

export const ICON_NAMES = Object.keys(ICONS) as IconName[];

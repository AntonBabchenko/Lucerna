import type { Row } from './installed-data.svelte';

// The display name for a row: the resolved platform project name when available,
// else the registry name (which for manual mods is the filename-derived name).
export const rowDisplayName = (r: Row): string => r.summary?.name ?? r.installed.name;

// Canonical cross-highlight / identity key for a mod. Platform mods use
// `source:project_id`; manual mods (source or project_id absent) fall back to
// `sha1:<sha1>` so they only highlight themselves.
export const modKey = (source: string | null, projectId: string | null, sha1: string): string =>
  source && projectId ? `${source}:${projectId}` : `sha1:${sha1}`;

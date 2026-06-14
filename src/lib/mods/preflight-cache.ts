import type { PreflightReport } from '$lib/ipc/bindings';
import { createLru } from './lru';

// Per-instance, process-lifetime LRU (cap 5) of the last pre-flight report.
export const preflightCache = createLru<PreflightReport>(5);

// The live half of the shader-loader hint: asks Modrinth whether a shader
// loader has a version for (mod loader, Minecraft), and the loader lists
// whether Lucerna can install a mod loader for that Minecraft. The only IPC of
// the hint; `shader-loader-hint.ts` decides what the answers mean.
//
// Each check is answered on its own and can never reject: `typedError` rethrows
// `Error` instances (a thrown IPC call, a missing command), and a throw is
// "could not tell", not a reason to strand the whole banner in pending.
//
// Answers are remembered for SUCCESS_TTL_MS, the same window the Rust loader
// lists cache for, so the banner stays within that window of what the mod
// detail modal (which reads the same command) would show. "Could not tell" is
// never remembered — the next look asks again (precedent: memory-bounds.ts).

import { commands } from '$lib/ipc/bindings';
import {
  type CheckPlan,
  type InstallAvailability,
  type InstallTarget,
  IRIS_MODRINTH_PROJECT_ID,
  OCULUS_MODRINTH_PROJECT_ID,
  type Outcomes,
  type ShaderAvailability,
  type ShaderCheck,
  shaderCheckKey,
} from './shader-loader-hint';

export const SUCCESS_TTL_MS = 5 * 60 * 1000;

const PROJECT_ID = {
  iris: IRIS_MODRINTH_PROJECT_ID,
  oculus: OCULUS_MODRINTH_PROJECT_ID,
} as const;

type Answer = ShaderAvailability | InstallAvailability;
const cache = new Map<string, { answer: Answer; at: number }>();

const shaderCacheKey = (c: ShaderCheck, mc: string) => `shader\n${c.shader}\n${c.loader}\n${mc}`;
const installCacheKey = (target: InstallTarget, mc: string) => `install\n${target}\n${mc}`;

function remembered(key: string): Answer | undefined {
  const entry = cache.get(key);
  if (!entry) return undefined;
  if (Date.now() - entry.at >= SUCCESS_TTL_MS) {
    cache.delete(key);
    return undefined;
  }
  return entry.answer;
}

function remember(key: string, answer: Answer): void {
  if (answer !== 'unknown') cache.set(key, { answer, at: Date.now() });
}

async function askShader(c: ShaderCheck, mc: string): Promise<ShaderAvailability> {
  try {
    const r = await commands.modsVersions('modrinth', PROJECT_ID[c.shader], mc, c.loader);
    if (r.status !== 'ok') return 'unknown';
    return r.data.length > 0 ? 'available' : 'no_build';
  } catch {
    // A thrown call (network layer, a missing command) means we could not tell.
    return 'unknown';
  }
}

function listLoaders(target: InstallTarget, mc: string) {
  switch (target) {
    case 'fabric':
      return commands.listFabricLoaders(mc);
    case 'neoforge':
      return commands.listNeoforgeLoaders(mc);
    case 'forge':
      return commands.listForgeLoaders(mc);
  }
}

async function askInstall(target: InstallTarget, mc: string): Promise<InstallAvailability> {
  try {
    const r = await listLoaders(target, mc);
    if (r.status === 'ok') return r.data.length > 0 ? 'installable' : 'not_installable';
    // `loader_unavailable` is the list coming back empty — the picker's own
    // "no <loader> for this Minecraft"; `loader_builds_not_offered` is Lucerna
    // deliberately not offering that loader for it (e.g. NeoForge on a
    // snapshot). Any other error — `loader_versions_unreadable` included — is
    // "could not tell".
    const kind = r.error.kind;
    return kind === 'loader_unavailable' || kind === 'loader_builds_not_offered'
      ? 'not_installable'
      : 'unknown';
  } catch {
    // A thrown call means we could not tell.
    return 'unknown';
  }
}

async function resolveShader(c: ShaderCheck, mc: string): Promise<ShaderAvailability> {
  const key = shaderCacheKey(c, mc);
  const known = remembered(key);
  if (known !== undefined) return known as ShaderAvailability;
  const answer = await askShader(c, mc);
  remember(key, answer);
  return answer;
}

async function resolveInstall(target: InstallTarget, mc: string): Promise<InstallAvailability> {
  const key = installCacheKey(target, mc);
  const known = remembered(key);
  if (known !== undefined) return known as InstallAvailability;
  const answer = await askInstall(target, mc);
  remember(key, answer);
  return answer;
}

/** Every answer the plan needs, when all are remembered and fresh; else `null`. */
export function cachedOutcomes(plan: CheckPlan, mc: string): Outcomes | null {
  const shader: Record<string, ShaderAvailability> = {};
  for (const c of plan.shader) {
    const known = remembered(shaderCacheKey(c, mc));
    if (known === undefined) return null;
    shader[shaderCheckKey(c)] = known as ShaderAvailability;
  }
  const install: Partial<Record<InstallTarget, InstallAvailability>> = {};
  for (const target of plan.install) {
    const known = remembered(installCacheKey(target, mc));
    if (known === undefined) return null;
    install[target] = known as InstallAvailability;
  }
  return { shader, install };
}

/** Answer every check of the plan, asking only for what is not remembered. Never rejects. */
export async function runChecks(plan: CheckPlan, mc: string): Promise<Outcomes> {
  const [shaderPairs, installPairs] = await Promise.all([
    Promise.all(
      plan.shader.map(async (c) => [shaderCheckKey(c), await resolveShader(c, mc)] as const),
    ),
    Promise.all(plan.install.map(async (t) => [t, await resolveInstall(t, mc)] as const)),
  ]);
  return { shader: Object.fromEntries(shaderPairs), install: Object.fromEntries(installPairs) };
}

/** Test-only: forget every remembered answer. */
export function __resetShaderLoaderAvailabilityCache(): void {
  cache.clear();
}

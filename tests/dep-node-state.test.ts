import { describe, expect, it } from 'vitest';
import type { DepProjectKey, DepTreeNode, DepViolation, PreflightReport } from '$lib/ipc/bindings';
import { classifyDepNode } from '$lib/mods/dep-node-state';
import { rawRangeDesc } from './test-utils/range-desc';

const node = (over: Partial<DepTreeNode> = {}): DepTreeNode => ({
  source: 'modrinth',
  project_id: 'PB',
  name: 'Balm',
  installed: false,
  declared: 'required',
  cycle: false,
  children: [],
  ...over,
});
const miss = (dependent: string, depId: string): DepViolation => ({
  kind: 'missing_required',
  dependent_name: dependent,
  dependent_sha1: dependent,
  dep_id: depId,
  needed: '',
  needed_desc: rawRangeDesc(''),
  installed_version: null,
  provider_project: null,
  provider_sha1: null,
  family: null,
});
const projects = (m: Record<string, DepProjectKey>) => (sha: string, depId: string) =>
  m[`${sha}:${depId}`] ?? null;
const classify = (
  report: PreflightReport | null,
  dependentSha1: string | null,
  over: Partial<Parameters<typeof classifyDepNode>[0]> = {},
) =>
  classifyDepNode({
    node: node(),
    dependentSha1,
    report,
    projectOf: () => null,
    ...over,
  });
// `dependent`'s own range rejects the installed Balm (project PB).
const outOfRange = (dependent: string): DepViolation => ({
  ...miss(dependent, 'balm'),
  kind: 'version_out_of_range',
  installed_version: '1.0',
  provider_project: { source: 'modrinth', project_id: 'PB', version_id: null },
  provider_sha1: 'balm-sha',
});

describe('classifyDepNode', () => {
  it('reads presence first: out of range, installed, disabled, optional', () => {
    const report = { violations: [outOfRange('a')] };
    expect(classify(report, 'a', { node: node({ installed: true }) })).toBe('out_of_range');
    expect(classify(null, 'a', { node: node({ installed: true }) })).toBe('installed');
    expect(classify(null, 'a', { node: node({ disabled: true }) })).toBe('disabled');
    expect(classify(null, 'a', { node: node({ declared: 'optional' }) })).toBe('optional_absent');
  });

  // Plan §5b V1 (06d): the mark is per edge. Another mod's range on the same project is that mod's
  // conflict; under this dependent the node is just installed.
  it('is out of range only under the dependent whose own range rejects it', () => {
    const report = { violations: [outOfRange('indium')] };
    const installed = node({ installed: true });
    expect(classify(report, 'indium', { node: installed })).toBe('out_of_range');
    expect(classify(report, 'iris', { node: installed })).toBe('installed');
    // Under an absent parent nothing installed declared the edge.
    expect(classify(report, null, { node: installed })).toBe('installed');
    // Another project under the same dependent is not the one its range names.
    expect(classify(report, 'indium', { node: node({ installed: true, project_id: 'PX' }) })).toBe(
      'installed',
    );
  });

  it('is loader-required only when THIS dependent’s missing id resolves here (A-F3)', () => {
    const report = { violations: [miss('a', 'balm'), miss('b', 'balm')] };
    const projectOf = projects({
      'a:balm': { source: 'modrinth', project_id: 'PB' },
      'b:balm': { source: 'modrinth', project_id: 'OTHER' },
    });
    expect(classify(report, 'a', { projectOf })).toBe('loader_required');
    expect(classify(report, 'b', { projectOf })).toBe('unknown');
  });

  it('is platform-only when the dependent was judged and the loader asked for nothing', () => {
    expect(classify({ violations: [] }, 'a')).toBe('platform_only');
    const other = { ...miss('a', 'x'), kind: 'version_out_of_range' as const };
    expect(classify({ violations: [other] }, 'a')).toBe('platform_only');
  });

  it('claims nothing about the loader without a verdict to stand on', () => {
    expect(classify({ violations: [], unjudged: ['a'] }, 'a')).toBe('unknown');
    expect(classify(null, 'a')).toBe('unknown');
    expect(classify({ violations: [] }, null)).toBe('unknown');
    expect(classify({ violations: [miss('a', 'balm')] }, 'a')).toBe('unknown'); // name unresolved
  });
});

// docs/BUILDING.md is the one build guide README and CONTRIBUTING point to.
// Its Linux package list and Node version are copied from the CI and release
// workflows, so a workflow change silently makes the guide wrong. These checks
// turn that drift into a red test.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const read = (path: string) => readFileSync(resolve(process.cwd(), path), 'utf8');

const building = read('docs/BUILDING.md');
const ci = read('.github/workflows/ci.yml');
const release = read('.github/workflows/release.yml');

// The lines of one top-level job (`  <name>:` up to the next job).
function jobBlock(workflow: string, job: string): string {
  const lines = workflow.split(/\r?\n/);
  const start = lines.indexOf(`  ${job}:`);
  if (start === -1) throw new Error(`job ${job} not found`);
  const end = lines.findIndex((line, i) => i > start && /^ {2}[\w-]+:\s*$/.test(line));
  return lines.slice(start, end === -1 ? undefined : end).join('\n');
}

// The text from the step with this name onwards.
function fromStep(text: string, name: string): string {
  const at = text.indexOf(`- name: ${name}`);
  if (at === -1) throw new Error(`step "${name}" not found`);
  return text.slice(at);
}

// The packages of the first `apt-get install -y` command, continuation lines included.
function aptPackages(text: string): string[] {
  const lines = text.split(/\r?\n/);
  let i = lines.findIndex((line) => /apt-get install -y\b/.test(line));
  if (i === -1) throw new Error('no apt-get install -y command found');
  let command = lines[i];
  while (command.trimEnd().endsWith('\\')) {
    i += 1;
    command = `${command.trimEnd().slice(0, -1)} ${lines[i]}`;
  }
  const words = command.trim().split(/\s+/);
  return words.slice(words.indexOf('-y') + 1).sort();
}

const linuxDeps = 'Install Tauri Linux deps';

describe('docs/BUILDING.md', () => {
  it('lists exactly the Linux packages the CI bundle job installs', () => {
    const ciPackages = aptPackages(fromStep(jobBlock(ci, 'linux-bundle'), linuxDeps));
    expect(ciPackages).toContain('libwebkit2gtk-4.1-dev');
    expect(aptPackages(building)).toEqual(ciPackages);
  });

  it('lists exactly the Linux packages the release build installs', () => {
    const releasePackages = aptPackages(fromStep(release, linuxDeps));
    expect(releasePackages).toContain('libwebkit2gtk-4.1-dev');
    expect(aptPackages(building)).toEqual(releasePackages);
  });

  it('names the Node.js major both workflows use', () => {
    const majors = [ci, release].flatMap((workflow) =>
      [...workflow.matchAll(/node-version:\s*['"]?(\d+)/g)].map((match) => match[1]),
    );
    expect(majors.length).toBeGreaterThan(0);
    expect([...new Set(majors)]).toEqual([majors[0]]);
    // The bold requirement itself: the guide also mentions newer majors in passing.
    expect(building).toContain(`**Node.js ${majors[0]}**`);
  });
});

describe('build instructions have one home', () => {
  it.each(['README.md', 'CONTRIBUTING.md'])('%s links to docs/BUILDING.md', (path) => {
    expect(read(path)).toContain('(docs/BUILDING.md)');
  });

  it.each([
    'README.md',
    'CONTRIBUTING.md',
    'docs/BUILDING.md',
  ])('%s leaves the pnpm version to package.json', (path) => {
    expect(read(path)).not.toContain('corepack prepare pnpm@');
  });
});

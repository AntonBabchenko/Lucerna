import { describe, expect, it } from 'vitest';
import { navVisual } from '$lib/layout/nav-status';

describe('navVisual', () => {
  it('running pulses green via the animation class, no wrench', () => {
    expect(navVisual('running')).toEqual({ iconClass: 'nav-icon-running', wrench: false });
  });

  it('crashed is red, no wrench', () => {
    expect(navVisual('crashed')).toEqual({ iconClass: 'text-danger', wrench: false });
  });

  it('fixable is amber with a wrench', () => {
    expect(navVisual('fixable')).toEqual({ iconClass: 'text-warning-text', wrench: true });
  });

  it('actionable (logs) is amber with a wrench', () => {
    expect(navVisual('actionable')).toEqual({ iconClass: 'text-warning-text', wrench: true });
  });

  it('advisory (logs) is amber, no wrench', () => {
    expect(navVisual('advisory')).toEqual({ iconClass: 'text-warning-text', wrench: false });
  });

  it('idle is neutral, no wrench', () => {
    expect(navVisual('idle')).toEqual({ iconClass: '', wrench: false });
  });
});

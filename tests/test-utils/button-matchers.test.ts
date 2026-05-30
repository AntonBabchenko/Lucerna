import { describe, expect, it } from 'vitest';

describe('toHaveBtnVariant matcher', () => {
  it('passes when className contains the expected btn-* class', () => {
    const btn = document.createElement('button');
    btn.className = 'btn-primary btn-sm';
    expect(btn).toHaveBtnVariant('primary');
  });

  it('fails with a diagnostic message when variant is missing', () => {
    const btn = document.createElement('button');
    btn.className = 'btn-secondary btn-sm';
    expect(() => expect(btn).toHaveBtnVariant('primary')).toThrow(
      /Expected button to have variant btn-primary/,
    );
  });

  it('supports negation with .not.toHaveBtnVariant', () => {
    const btn = document.createElement('button');
    btn.className = 'px-3 py-2 text-base border-b-2 -mb-px'; // underlined-tab style
    expect(btn).not.toHaveBtnVariant('secondary');
  });
});

describe('toHaveBtnSize matcher', () => {
  it('passes when className contains the expected btn-sm/xs/lg class', () => {
    const btn = document.createElement('button');
    btn.className = 'btn-primary btn-lg';
    expect(btn).toHaveBtnSize('lg');
  });

  it('fails when size mismatches', () => {
    const btn = document.createElement('button');
    btn.className = 'btn-primary btn-sm';
    expect(() => expect(btn).toHaveBtnSize('lg')).toThrow(/Expected button to have size btn-lg/);
  });
});

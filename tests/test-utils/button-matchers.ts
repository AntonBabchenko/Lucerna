import { expect } from 'vitest';

type Variant =
  | 'primary'
  | 'success'
  | 'danger'
  | 'secondary'
  | 'tertiary'
  | 'warning'
  | 'ghost'
  | 'ghost-danger'
  | 'icon'
  | 'link';
type Size = 'xs' | 'sm' | 'lg';

function classList(el: unknown): string[] {
  if (
    el &&
    typeof el === 'object' &&
    'className' in el &&
    typeof (el as { className: unknown }).className === 'string'
  ) {
    return ((el as { className: string }).className || '').split(/\s+/).filter(Boolean);
  }
  return [];
}

expect.extend({
  toHaveBtnVariant(received: unknown, variant: Variant) {
    const classes = classList(received);
    const expected = `btn-${variant}`;
    const pass = classes.includes(expected);
    return {
      pass,
      message: () =>
        pass
          ? `Expected button NOT to have variant ${expected}; className was "${classes.join(' ')}"`
          : `Expected button to have variant ${expected}; className was "${classes.join(' ')}"`,
    };
  },
  toHaveBtnSize(received: unknown, size: Size) {
    const classes = classList(received);
    const expected = `btn-${size}`;
    const pass = classes.includes(expected);
    return {
      pass,
      message: () =>
        pass
          ? `Expected button NOT to have size ${expected}; className was "${classes.join(' ')}"`
          : `Expected button to have size ${expected}; className was "${classes.join(' ')}"`,
    };
  },
});

declare module 'vitest' {
  // Must match vitest's own `interface Assertion<T>` — no default — to satisfy
  // "all declarations must have identical type parameters".
  interface Assertion<T> {
    toHaveBtnVariant(variant: Variant): T;
    toHaveBtnSize(size: Size): T;
  }
  interface AsymmetricMatchersContaining {
    toHaveBtnVariant(variant: Variant): unknown;
    toHaveBtnSize(size: Size): unknown;
  }
}

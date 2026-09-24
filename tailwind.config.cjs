/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ['./src/app.html', './src/**/*.{svelte,ts,js}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        // Surfaces
        base: 'rgb(var(--bg-base) / <alpha-value>)',
        surface: 'rgb(var(--bg-surface) / <alpha-value>)',
        subtle: 'rgb(var(--bg-subtle) / <alpha-value>)',
        muted: 'rgb(var(--bg-muted) / <alpha-value>)',
        // Segmented control track and chosen-segment thumb (DESIGN.md §6).
        'control-track': 'rgb(var(--control-track) / <alpha-value>)',
        'control-thumb': 'rgb(var(--control-thumb) / <alpha-value>)',
        // Borders
        'border-subtle': 'rgb(var(--border-subtle) / <alpha-value>)',
        'border-emphasis': 'rgb(var(--border-emphasis) / <alpha-value>)',
        // Accent / state
        accent: 'rgb(var(--accent) / <alpha-value>)',
        'accent-soft': 'rgb(var(--accent-soft) / <alpha-value>)',
        success: 'rgb(var(--success) / <alpha-value>)',
        'success-bg': 'rgb(var(--success-bg) / <alpha-value>)',
        danger: 'rgb(var(--danger) / <alpha-value>)',
        'danger-bg': 'rgb(var(--danger-bg) / <alpha-value>)',
        'warning-bg': 'rgb(var(--warning-bg) / <alpha-value>)',
        'warning-text': 'rgb(var(--warning-text) / <alpha-value>)',
      },
      // Note: textColor overrides Tailwind's colors map for text-*
      // utilities, so `text-muted` here resolves to --text-muted
      // (not colors.muted's --bg-muted). The bare-key naming
      // (primary, secondary, muted, placeholder) is the right
      // shape — using `text-X` as the key would generate the
      // surprising `text-text-X` utility.
      textColor: {
        primary: 'rgb(var(--text-primary) / <alpha-value>)',
        secondary: 'rgb(var(--text-secondary) / <alpha-value>)',
        muted: 'rgb(var(--text-muted) / <alpha-value>)',
        placeholder: 'rgb(var(--text-placeholder) / <alpha-value>)',
        // Deliberately text-only: bg-danger / border-danger / .btn-danger keep
        // resolving through colors.danger, so this changes foreground text
        // without repainting a single fill.
        danger: 'rgb(var(--danger-text) / <alpha-value>)',
        // The same split for the two tones whose fill tier fails AA as text:
        // accent in dark (2.8:1 on --accent-soft), success in both themes
        // (3.1:1 light / 4.0:1 dark on --success-bg). bg-accent / border-accent
        // / outline-accent / .btn-primary and bg-success / border-success /
        // .btn-success keep the fill tokens through `colors`; only text-accent
        // and text-success move. tests/toast-contrast.test.ts holds the pairing.
        accent: 'rgb(var(--accent-text) / <alpha-value>)',
        success: 'rgb(var(--success-text) / <alpha-value>)',
      },
      // Default border color follows the theme token so bare `border`
      // / `border-b` / `border-t` utilities pick up the dark-palette
      // colour instead of Tailwind's hardcoded #e5e7eb preflight
      // default (which produced a stuck light-grey hairline in dark
      // mode across ~60 sites).
      borderColor: {
        DEFAULT: 'rgb(var(--border-subtle) / <alpha-value>)',
      },
    },
  },
  plugins: [],
};

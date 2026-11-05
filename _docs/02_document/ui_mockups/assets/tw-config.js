// Tailwind CDN theme for the mockups: colour utilities resolve to the CSS tokens in tokens.css.
tailwind.config = {
  theme: {
    extend: {
      colors: {
        page: "var(--bg-page)",
        surface: "var(--bg-surface)",
        raised: "var(--bg-raised)",
        sunken: "var(--bg-sunken)",
        ink: { DEFAULT: "var(--text-primary)", 2: "var(--text-secondary)", 3: "var(--text-tertiary)" },
        line: { DEFAULT: "var(--border)", strong: "var(--border-strong)" },
        action: { DEFAULT: "var(--action)", subtle: "var(--action-subtle)", on: "var(--on-action)", "on-subtle": "var(--on-action-subtle)" },
        link: "var(--link)",
        sun: { DEFAULT: "var(--sun)", strong: "var(--sun-strong)", fill: "var(--sun-fill)", on: "var(--on-sun)" },
        success: { DEFAULT: "var(--success)", bg: "var(--success-bg)" },
        warning: { DEFAULT: "var(--warning)", bg: "var(--warning-bg)", on: "var(--on-warning-bg)" },
        danger: { DEFAULT: "var(--danger)", bg: "var(--danger-bg)", on: "var(--on-danger-bg)" },
      },
      fontFamily: { ui: "var(--font-ui)", mono: "var(--font-mono)" },
      borderRadius: { sm: "var(--radius-sm)", md: "var(--radius-md)", lg: "var(--radius-lg)" },
    },
  },
};

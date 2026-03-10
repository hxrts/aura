/** @type {import('tailwindcss').Config} */
const userHome = process.env.HOME || process.env.USERPROFILE || '';
const dioxusShadcnGlob = userHome
  ? `${userHome}/.cargo/git/checkouts/dioxus-shadcn-*/**/blocks/src/**/*.rs`
  : null;

module.exports = {
  darkMode: ['class'],
  content: [
    './index.html',
    './src/**/*.{rs,html}',
    '../aura-ui/src/**/*.rs',
    ...(dioxusShadcnGlob ? [dioxusShadcnGlob] : [])
  ],
  theme: {
    extend: {
      fontFamily: {
        sans: ['var(--font-sans)'],
        mono: ['var(--font-mono)']
      },
      borderRadius: {
        lg: 'var(--radius)',
        md: 'calc(var(--radius) - 2px)',
        sm: '0.125rem'
      },
      colors: {
        background: 'oklch(from var(--background) l c h / <alpha-value>)',
        foreground: 'oklch(from var(--foreground) l c h / <alpha-value>)',
        card: {
          DEFAULT: 'oklch(from var(--card) l c h / <alpha-value>)',
          foreground: 'oklch(from var(--card-foreground) l c h / <alpha-value>)'
        },
        popover: {
          DEFAULT: 'oklch(from var(--popover) l c h / <alpha-value>)',
          foreground: 'oklch(from var(--popover-foreground) l c h / <alpha-value>)'
        },
        primary: {
          DEFAULT: 'oklch(from var(--primary) l c h / <alpha-value>)',
          foreground: 'oklch(from var(--primary-foreground) l c h / <alpha-value>)'
        },
        secondary: {
          DEFAULT: 'oklch(from var(--secondary) l c h / <alpha-value>)',
          foreground: 'oklch(from var(--secondary-foreground) l c h / <alpha-value>)'
        },
        muted: {
          DEFAULT: 'oklch(from var(--muted) l c h / <alpha-value>)',
          foreground: 'oklch(from var(--muted-foreground) l c h / <alpha-value>)'
        },
        accent: {
          DEFAULT: 'oklch(from var(--accent) l c h / <alpha-value>)',
          foreground: 'oklch(from var(--accent-foreground) l c h / <alpha-value>)'
        },
        destructive: {
          DEFAULT: 'oklch(from var(--destructive) l c h / <alpha-value>)',
          foreground: 'oklch(from var(--destructive-foreground) l c h / <alpha-value>)'
        },
        border: 'oklch(from var(--border) l c h / <alpha-value>)',
        input: 'oklch(from var(--input) l c h / <alpha-value>)',
        ring: 'oklch(from var(--ring) l c h / <alpha-value>)'
      }
    }
  },
  plugins: []
};

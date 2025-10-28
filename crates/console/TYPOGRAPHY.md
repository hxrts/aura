# Typography System

## Overview

The Aura console uses a **logical, semantic typography hierarchy** with the Perun font family. All typography is defined through CSS classes in `tailwind.css` - no inline font utilities should be used.

## Font Family

**Primary:** Perun (sans-serif)
- Regular (400)
- Medium (500) 
- Semibold (600)
- Bold (700)
- Italic variants available

**Monospace:** System default for code/REPL

## Typography Scale

### Display Scale (App/Page Level)

| Class | Size | Weight | Usage |
|-------|------|--------|-------|
| `.display-1` | 24px (2xl) | Bold (700) | App title, hero text |
| `.display-2` | 20px (xl) | Bold (700) | Page titles |

### Heading Scale (Content Structure)

| Class | Size | Weight | Usage |
|-------|------|--------|-------|
| `.heading-1` | 18px (lg) | Semibold (600) | Major section titles |
| `.heading-2` | 16px (base) | Semibold (600) | Subsection titles |
| `.heading-3` | 14px (sm) | Semibold (600) | Card/panel titles |
| `.heading-4` | 12px (xs) | Semibold (600) + uppercase | Labels, overlines |

### Body Scale (Content)

| Class | Size | Weight | Usage |
|-------|------|--------|-------|
| `.body-lg` | 16px (base) | Regular (400) | Emphasized body text |
| `.body` | 14px (sm) | Regular (400) | Standard body text |
| `.body-sm` | 12px (xs) | Regular (400) | Secondary text, metadata |
| `.caption` | 12px (xs) | Regular (400) | Muted captions, timestamps |

### UI Scale (Interactive Elements)

| Class | Size | Weight | Usage |
|-------|------|--------|-------|
| `.ui-button` | 14px (sm) | Medium (500) | Standard buttons |
| `.ui-button-sm` | 12px (xs) | Medium (500) | Small buttons |
| `.ui-input` | 14px (sm) | Regular (400) | Form inputs |
| `.ui-label` | 12px (xs) | Medium (500) | Form labels |
| `.ui-badge` | 12px (xs) | Medium (500) | Status badges |

### Code Scale (Monospace)

| Class | Size | Weight | Usage |
|-------|------|--------|-------|
| `.code` | 12px (xs) | Monospace | All code/REPL text |

## Component Classes

Pre-built component classes that include typography:

```css
/* Buttons */
.btn-primary        /* ui-button + styling */
.btn-secondary      /* ui-button + styling */
.btn-success        /* ui-button-sm + styling */
.btn-danger         /* ui-button-sm + styling */
.btn-sm             /* ui-button size variant */
.btn-icon           /* ui-button + styling */

/* Inputs */
.input-base         /* ui-input + styling */
.input-lg           /* body-lg variant */
.input-sm           /* body-sm variant */

/* Status Badges */
.status-badge-connected      /* ui-badge + styling */
.status-badge-disconnected   /* ui-badge + styling */

/* Code */
.code-block         /* code + block styling */
.code-inline        /* code + inline styling */
.code-prompt        /* code + prompt color */
.code-text          /* code + text color */
.code-error         /* code + error color */
.code-output        /* code + output color */

/* Branch Components */
.branch-title       /* heading-3 + truncate */
.branch-meta        /* body-sm */

/* Modal */
.modal-header h4    /* heading-1 */
.modal-body p       /* body-sm */
.modal-input        /* ui-input + styling */

/* Header */
.header-title       /* display-2 (mobile), display-1 (desktop) */
```

## Usage Guidelines

### Do This

```rust
// Use semantic typography classes
view! {
    <h3 class="heading-1">"Section Title"</h3>
    <p class="body">"Body text goes here"</p>
    <button class="btn-primary">"Click Me"</button>
}
```

### Don't Do This

```rust
// Don't use inline Tailwind typography utilities
view! {
    <h3 class="text-lg font-semibold">"Section Title"</h3>
    <p class="text-sm">"Body text"</p>
    <button class="text-sm font-medium">"Click Me"</button>
}
```

## Hierarchy Examples

### Section Pattern
```rust
<h2 class="heading-1">"Timeline"</h2>
<div class="card">
    <h3 class="heading-3">"Event Details"</h3>
    <p class="body-sm">"Event occurred at..."</p>
    <span class="caption">"2 hours ago"</span>
</div>
```

### Form Pattern
```rust
<label class="ui-label">"Name"</label>
<input class="input-base" />
<p class="body-sm">"Enter your full name"</p>
```

### List Pattern
```rust
<div class="branch-item">
    <div class="branch-title">"Main Branch"</div>
    <div class="branch-meta">"Last updated 5m ago"</div>
</div>
```

## Color Utilities (Separate from Typography)

Typography classes focus on size and weight. Use semantic color classes separately:

```css
.text-primary       /* Primary text color */
.text-secondary     /* Secondary text color */
.text-tertiary      /* Tertiary text color */
.highlight          /* Highlight color */
```

## Migration Checklist

When updating components:

1. Replace inline `text-*` utilities with semantic classes
2. Replace inline `font-*` utilities with semantic classes  
3. Use component classes (`.btn-primary`, `.input-base`) when available
4. Combine typography class with separate color class if needed
5. Update legacy aliases to new class names

## Design Tokens Reference

| Token | Value (px) | Value (rem) | Tailwind |
|-------|-----------|-------------|----------|
| xs | 12px | 0.75rem | text-xs |
| sm | 14px | 0.875rem | text-sm |
| base | 16px | 1rem | text-base |
| lg | 18px | 1.125rem | text-lg |
| xl | 20px | 1.25rem | text-xl |
| 2xl | 24px | 1.5rem | text-2xl |

## Questions?

For questions or proposed changes to the typography system, consult the design team or open an issue.

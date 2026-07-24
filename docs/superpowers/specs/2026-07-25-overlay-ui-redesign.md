# Overlay UI Redesign

**Date:** 2026-07-25
**Scope:** `DropHandle` and `DropPanel` components — CSS only, no logic changes
**Files:** `src/App.css` (overlay section, lines 658–866), `src/App.tsx` (JSX structure for DropHandle/DropPanel), `src/lib/overlay.ts` (size constants)

---

## Background

The floating drop overlay consists of two transparent Tauri windows:

- **`drop-handle`** — a small tab that lives on the screen edge at all times. The user can hover or drag files onto it to expand the panel. Currently 44×12px (or 12×44px) pill.
- **`drop-panel`** — the expanded 264×220px card that appears when the handle is hovered or a file drag enters it. Contains a device selector, dropzone, and error display.

Previous sessions fixed the drag-detection regression (native cursor polling), removed border artifacts, and aligned colors to CSS variables. This spec defines the visual redesign that comes next.

---

## Design Decisions

| Question | Choice | Rationale |
|---|---|---|
| Overall style | **Solid Elevated** | Clean card consistent with main window; no glassmorphism complexity |
| Device selection | **Keep `<select>` dropdown** | Only 1–2 devices in practice; no structural redesign needed |
| Handle tab | **C — larger, rounder, with chevron icon** | More discoverable, easier to click, stronger visual identity |
| Dropzone | **A — refined dashed border** | Familiar affordance; activate state adds solid border + accent fill |

---

## Component Specs

### 1. Handle Tab (collapsed state)

**Goal:** More prominent, clearly interactive, edge-adaptive.

| Property | Value |
|---|---|
| Size (left/right edge) | 20px wide × 52px tall |
| Size (top/bottom edge) | 52px wide × 20px tall |
| `border-radius` (right edge) | `10px 0 0 10px` |
| `border-radius` (left edge) | `0 10px 10px 0` |
| `border-radius` (bottom edge) | `10px 10px 0 0` |
| `border-radius` (top edge) | `0 0 10px 10px` |
| Background | `var(--color-accent)` |
| Shadow (right edge) | `-3px 0 16px rgba(22,114,184,0.38)` |
| Shadow (left edge) | `3px 0 16px rgba(22,114,184,0.38)` |
| Shadow (bottom edge) | `0 -3px 16px rgba(22,114,184,0.38)` |
| Shadow (top edge) | `0 3px 16px rgba(22,114,184,0.38)` |
| Dark-mode shadow | Same with `rgba(78,161,224,0.32)` |
| Icon | SVG chevron (`<`) pointing away from screen edge, `white`, opacity `0.75`, 9×14px |
| Breathing animation | Keep `overlay-handle-breathe` (opacity 0.82→1) |
| Hover/drag-active | `animation-play-state: paused; filter: brightness(1.12)` |

The chevron direction adapts per edge:
- Right edge → points left (`←`)
- Left edge → points right (`→`)
- Bottom edge → points up (`↑`)
- Top edge → points down (`↓`)

**Size constant changes in `src/lib/overlay.ts`:**
```
OVERLAY_HANDLE_LENGTH = 52   // was 44
OVERLAY_HANDLE_THICKNESS = 20  // was 12
```
And update matching initial window sizes in `src-tauri/tauri.conf.json`:
```json
"width": 20, "height": 52   // drop-handle window — startup default edge is 'right' (THICKNESS × LENGTH)
```

### 2. Panel Card (expanded state)

**Goal:** Clean elevated card matching main window style, with refined spacing.

| Property | Value |
|---|---|
| Panel window size | Keep 264×220 (no change) |
| Root padding | `8px` (keeps shadow inside window bounds) |
| Card `border-radius` | `12px` |
| Card background | `var(--color-surface)` |
| Card border | `none` |
| Card shadow (light) | `0 4px 24px rgba(24,34,45,0.12), 0 1px 4px rgba(24,34,45,0.06)` |
| Card shadow (dark) | `0 4px 24px rgba(0,0,0,0.42), 0 1px 4px rgba(0,0,0,0.22)` |
| Card padding | `12px` |

### 3. Panel Header

No structural change. Refine styling only:

| Property | Value |
|---|---|
| Font size | `10px` |
| Font weight | `700` |
| Text transform | `uppercase` |
| Letter spacing | `0.07em` |
| Color | `var(--color-text-muted)` |
| Margin bottom | `9px` |
| Grip icon (`⠿`) | `var(--color-text-faint)`, `font-size: 16px` |

### 4. Device Select

| Property | Value |
|---|---|
| Background | `var(--color-surface-muted)` |
| Border | `1px solid var(--color-border)` |
| Border radius | `7px` |
| Padding | `5px 9px` |
| Font size | `11px` |
| Color | `var(--color-text)` |
| Margin bottom | `8px` |
| Width | `100%` |

Override the global `button, select { min-height: 38px }` rule for `.overlay-select` by setting `min-height: 0`.

### 5. Dropzone

**Static state:**

| Property | Value |
|---|---|
| Border | `1.5px dashed var(--color-border)` |
| Border radius | `10px` |
| Background | `var(--color-surface-muted)` |
| Padding | `16px 8px` |
| Icon font size | `22px` |
| Label font size | `10px`, weight `600`, color `var(--color-text-muted)` |

**Active (drag-over) state** (`.overlay-dropzone-active`):

| Property | Value |
|---|---|
| Border | `2px solid var(--color-accent)` (solid) |
| Background | `var(--color-accent-bg)` |
| Card outer glow (light) | additional `box-shadow` on `.overlay-card`: `0 0 0 3px rgba(22,114,184,0.15)` |
| Card outer glow (dark) | `0 0 0 3px rgba(78,161,224,0.20)` |
| Label text | changes to "松手即发送" (handled in JSX already) |
| Label color | `var(--color-accent)`, weight `700` |

Transition on border, background, and color: `160ms ease`.

### 6. Error Display

No change to current implementation. Color stays `var(--color-danger)`, font `10px`.

---

## CSS Changes Summary

All changes are confined to the overlay section of `src/App.css` (after the `/* ── Drop Overlay Window ── */` comment).

**Key diffs:**
1. `.overlay-handle-button` → width/height changed to fill the new 20px thickness
2. Edge-specific `border-radius` rules updated for 10px radius
3. Edge-specific `box-shadow` rules added (directional based on edge)
4. SVG chevron icon added inside `.overlay-handle-button` via a new `<svg>` element in JSX (replaces the hidden `.overlay-handle-icon` div)
5. `.overlay-card` shadow values updated (light + dark media query)
6. `.overlay-select` min-height override added
7. `.overlay-dropzone` padding/border-radius refined
8. `.overlay-panel-root.overlay-panel-dragging .overlay-card` glow updated

## Constants Changes

`src/lib/overlay.ts`:
```ts
export const OVERLAY_HANDLE_LENGTH = 52
export const OVERLAY_HANDLE_THICKNESS = 20
```

`src-tauri/tauri.conf.json` drop-handle window (startup edge is `'right'`, so THICKNESS=width, LENGTH=height):
```json
"width": 20, "height": 52
```

---

## JSX Changes

`DropHandle` component in `src/App.tsx`:

Replace the empty `<div className="overlay-handle-icon" />` (or the plain button body) with an SVG chevron. The chevron direction is determined by `handleEdge` state:

```tsx
function chevronForEdge(edge: OverlayEdge) {
  // Returns SVG path d attribute pointing away from the screen edge
  // right → left-pointing chevron: "M6.5 2L2 7l4.5 5"
  // left  → right-pointing: "M2.5 2L7 7l-4.5 5"
  // bottom → up-pointing: "M2 6.5L7 2l5 4.5"
  // top   → down-pointing: "M2 2.5L7 7l-5 4.5" (approx, adjust viewBox)
}
```

The SVG is `width="9" height="14"` for left/right edges, `width="14" height="9"` (transposed) for top/bottom edges. `fill="none"`, `stroke="white"`, `strokeWidth="1.6"`, `strokeLinecap="round"`, `strokeLinejoin="round"`, `opacity="0.75"`.

---

## Out of Scope

- No changes to drag detection logic, cursor polling, or event coordination
- No changes to `DropPanel` layout structure (header / select / dropzone order)
- No changes to `TransferPanel` or main `App` component
- No new features (no transfer progress in overlay, no pin button, etc.)

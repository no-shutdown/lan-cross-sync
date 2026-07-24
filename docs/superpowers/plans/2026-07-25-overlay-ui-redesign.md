# Overlay UI Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redesign the floating drop overlay (handle tab + panel card) with a solid elevated visual style, a larger chevron-icon handle tab, and refined dropzone states — CSS-only changes plus minor JSX/constant updates.

**Architecture:** All visual changes are confined to the overlay section of `src/App.css` (after the `/* ── Drop Overlay Window ── */` comment). Size constant changes in `src/lib/overlay.ts` automatically propagate through the existing positioning math. A small JSX change in `src/App.tsx` adds the SVG chevron icon to the handle button.

**Tech Stack:** React + TypeScript (Tauri v2), CSS custom properties (`var(--color-*)` tokens already defined in `:root`)

**Spec:** `docs/superpowers/specs/2026-07-25-overlay-ui-redesign.md`

---

## Files

| File | Change |
|---|---|
| `src/lib/overlay.ts` | Update `OVERLAY_HANDLE_LENGTH` (44→52) and `OVERLAY_HANDLE_THICKNESS` (12→20) |
| `src-tauri/tauri.conf.json` | Update `drop-handle` initial window size (44×12 → 20×52) |
| `src/App.tsx` | Add `chevronForEdge()` helper; add SVG inside handle button; update dropzone label for drag-over state |
| `src/App.css` | Rewrite overlay section: handle border-radius (10px), edge-specific shadows, panel card shadow/radius, select border-radius, dropzone padding/radius/active |

---

## Task 1: Update Handle Size Constants

**Files:**
- Modify: `src/lib/overlay.ts:4-5`
- Modify: `src-tauri/tauri.conf.json:22-25`

- [ ] **Step 1: Update constants in `src/lib/overlay.ts`**

Change lines 4–5:
```ts
export const OVERLAY_HANDLE_LENGTH = 52
export const OVERLAY_HANDLE_THICKNESS = 20
```
`OVERLAY_HANDLE_W` and `OVERLAY_HANDLE_H` are derived aliases — no change needed. The cursor-polling math and `handleSizeForEdge()` all read from these, so they update automatically.

- [ ] **Step 2: Update initial window size in `src-tauri/tauri.conf.json`**

The `drop-handle` window entry (currently `"width": 44, "height": 12`). Startup default edge is `'right'`, so thickness (20) is width and length (52) is height:
```json
{
  "label": "drop-handle",
  "title": "",
  "width": 20,
  "height": 52,
  "decorations": false,
  "transparent": true,
  "alwaysOnTop": true,
  "skipTaskbar": true,
  "visible": false,
  "focus": false,
  "resizable": false
}
```

- [ ] **Step 3: Commit**

```bash
git add src/lib/overlay.ts src-tauri/tauri.conf.json
git commit -m "feat: increase handle tab size to 20×52px"
```

---

## Task 2: Handle Tab CSS — Border Radius and Shadow

**Files:**
- Modify: `src/App.css` (overlay section, edge-specific rules and `.overlay-handle-button`)

- [ ] **Step 1: Replace edge-specific border-radius rules**

Find the 4 edge-specific rules (currently using `999px` for the rounded corners):
```css
.overlay-handle-root.overlay-edge-right  .overlay-handle-button { border-radius: 999px 0 0 999px; }
.overlay-handle-root.overlay-edge-left   .overlay-handle-button { border-radius: 0 999px 999px 0; }
.overlay-handle-root.overlay-edge-bottom .overlay-handle-button { border-radius: 999px 999px 0 0; }
.overlay-handle-root.overlay-edge-top    .overlay-handle-button { border-radius: 0 0 999px 999px; }
```

Replace with (10px on visible corners, 0 on screen-edge corners):
```css
.overlay-handle-root.overlay-edge-right  .overlay-handle-button { border-radius: 10px 0 0 10px; }
.overlay-handle-root.overlay-edge-left   .overlay-handle-button { border-radius: 0 10px 10px 0; }
.overlay-handle-root.overlay-edge-bottom .overlay-handle-button { border-radius: 10px 10px 0 0; }
.overlay-handle-root.overlay-edge-top    .overlay-handle-button { border-radius: 0 0 10px 10px; }
```

- [ ] **Step 2: Add edge-specific box-shadow rules**

Add below the border-radius rules:
```css
.overlay-handle-root.overlay-edge-right  .overlay-handle-button { box-shadow: -3px 0 16px rgba(22, 114, 184, 0.38); }
.overlay-handle-root.overlay-edge-left   .overlay-handle-button { box-shadow:  3px 0 16px rgba(22, 114, 184, 0.38); }
.overlay-handle-root.overlay-edge-bottom .overlay-handle-button { box-shadow:  0 -3px 16px rgba(22, 114, 184, 0.38); }
.overlay-handle-root.overlay-edge-top    .overlay-handle-button { box-shadow:  0  3px 16px rgba(22, 114, 184, 0.38); }

@media (prefers-color-scheme: dark) {
  .overlay-handle-root.overlay-edge-right  .overlay-handle-button { box-shadow: -3px 0 16px rgba(78, 161, 224, 0.32); }
  .overlay-handle-root.overlay-edge-left   .overlay-handle-button { box-shadow:  3px 0 16px rgba(78, 161, 224, 0.32); }
  .overlay-handle-root.overlay-edge-bottom .overlay-handle-button { box-shadow:  0 -3px 16px rgba(78, 161, 224, 0.32); }
  .overlay-handle-root.overlay-edge-top    .overlay-handle-button { box-shadow:  0  3px 16px rgba(78, 161, 224, 0.32); }
}
```

**Important:** The current `.overlay-handle-button` rule has `box-shadow: none` — this is now overridden by the more-specific edge rules above. Remove `box-shadow: none` from the base `.overlay-handle-button` rule so the cascade works correctly (the edge rules win).

- [ ] **Step 3: Commit**

```bash
git add src/App.css
git commit -m "feat: update handle tab border-radius to 10px and add directional shadows"
```

---

## Task 3: SVG Chevron Icon in Handle JSX

**Files:**
- Modify: `src/App.tsx` — add `chevronForEdge()` helper before `DropHandle`, update the button's JSX body

- [ ] **Step 1: Add `chevronForEdge` helper function**

Add this function just before the `DropHandle` export (around line 143, before `export function DropHandle`):
```tsx
function chevronForEdge(edge: OverlayEdge) {
  if (edge === 'left' || edge === 'right') {
    const d = edge === 'right' ? 'M6.5 2L2 7l4.5 5' : 'M2.5 2L7 7l-4.5 5'
    return (
      <svg width="9" height="14" viewBox="0 0 9 14" fill="none" aria-hidden="true" style={{ opacity: 0.75 }}>
        <path d={d} stroke="white" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
    )
  }
  const d = edge === 'bottom' ? 'M2 6.5L7 2l5 4.5' : 'M2 2.5L7 7l5-4.5'
  return (
    <svg width="14" height="9" viewBox="0 0 14 9" fill="none" aria-hidden="true" style={{ opacity: 0.75 }}>
      <path d={d} stroke="white" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  )
}
```

- [ ] **Step 2: Update the handle button to render the chevron**

In the `DropHandle` return, the button is currently a self-closing div:
```tsx
<div
  role="button"
  tabIndex={0}
  className="overlay-handle-button"
  aria-label={t('zh-CN', 'dropTitle')}
  title={t('zh-CN', 'dropTitle')}
  onMouseEnter={handlePointerEnter}
  onMouseLeave={handlePointerLeave}
  onKeyDown={handleKeyDown}
  onPointerDown={startWindowDrag}
  onClick={() => void openPanel()}
/>
```

Change to an open element containing the chevron:
```tsx
<div
  role="button"
  tabIndex={0}
  className="overlay-handle-button"
  aria-label={t('zh-CN', 'dropTitle')}
  title={t('zh-CN', 'dropTitle')}
  onMouseEnter={handlePointerEnter}
  onMouseLeave={handlePointerLeave}
  onKeyDown={handleKeyDown}
  onPointerDown={startWindowDrag}
  onClick={() => void openPanel()}
>
  {chevronForEdge(handleEdge)}
</div>
```

The `handleEdge` state already exists (line 161) and updates when the handle snaps to an edge. The `.overlay-handle-icon` div below the button div is now unused — delete it:
```tsx
// DELETE this line entirely:
<div className="overlay-handle-icon" />
```

- [ ] **Step 3: Commit**

```bash
git add src/App.tsx
git commit -m "feat: add edge-adaptive chevron icon to handle tab"
```

---

## Task 4: Panel Card and Header CSS

**Files:**
- Modify: `src/App.css` — `.overlay-card`, `.overlay-header`, `.overlay-drag-grip`, drag-active glow

- [ ] **Step 1: Update `.overlay-card` base styles**

Current:
```css
.overlay-card {
  animation: overlay-panel-enter 180ms cubic-bezier(0.2, 0.8, 0.2, 1);
  background: var(--color-surface);
  border: none;
  border-radius: 10px;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.1), 0 1px 3px rgba(0, 0, 0, 0.06);
  ...
}
```

Update `border-radius` and `box-shadow`:
```css
.overlay-card {
  animation: overlay-panel-enter 180ms cubic-bezier(0.2, 0.8, 0.2, 1);
  background: var(--color-surface);
  border: none;
  border-radius: 12px;
  box-shadow: 0 4px 24px rgba(24, 34, 45, 0.12), 0 1px 4px rgba(24, 34, 45, 0.06);
  box-sizing: border-box;
  height: 100%;
  overflow: hidden;
  padding: 12px;
  position: relative;
  width: 100%;
  z-index: 1;
}
```

- [ ] **Step 2: Update dark-mode card shadow**

Find the existing dark-mode block for `.overlay-card`:
```css
@media (prefers-color-scheme: dark) {
  .overlay-card {
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.4), 0 1px 3px rgba(0, 0, 0, 0.25);
  }
}
```

Replace with:
```css
@media (prefers-color-scheme: dark) {
  .overlay-card {
    box-shadow: 0 4px 24px rgba(0, 0, 0, 0.42), 0 1px 4px rgba(0, 0, 0, 0.22);
  }
}
```

- [ ] **Step 3: Update drag-active card glow**

Find:
```css
.overlay-panel-root.overlay-panel-dragging .overlay-card {
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.1), 0 0 0 3px rgba(22, 114, 184, 0.18);
}

@media (prefers-color-scheme: dark) {
  .overlay-panel-root.overlay-panel-dragging .overlay-card {
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.4), 0 0 0 3px rgba(78, 161, 224, 0.25);
  }
}
```

Replace with:
```css
.overlay-panel-root.overlay-panel-dragging .overlay-card {
  box-shadow: 0 4px 24px rgba(24, 34, 45, 0.12), 0 0 0 3px rgba(22, 114, 184, 0.15);
}

@media (prefers-color-scheme: dark) {
  .overlay-panel-root.overlay-panel-dragging .overlay-card {
    box-shadow: 0 4px 24px rgba(0, 0, 0, 0.42), 0 0 0 3px rgba(78, 161, 224, 0.20);
  }
}
```

- [ ] **Step 4: Refine `.overlay-header` and `.overlay-drag-grip`**

In `.overlay-header`, change `margin-bottom: 8px` → `margin-bottom: 9px`.

In `.overlay-drag-grip`, change `font-size: 17px` → `font-size: 16px`.

- [ ] **Step 5: Commit**

```bash
git add src/App.css
git commit -m "feat: refine panel card shadow, border-radius, and header styling"
```

---

## Task 5: Select, Dropzone, and Active Label CSS + JSX

**Files:**
- Modify: `src/App.css` — `.overlay-select`, `.overlay-dropzone`, `.overlay-dropzone-active`, `.overlay-drop-label`
- Modify: `src/App.tsx` — dropzone label text in `DropPanel`

- [ ] **Step 1: Update `.overlay-select`**

Add explicit `border-radius: 7px` and update padding (the global `select` rule sets `border-radius: 6px`, so we override):

Current:
```css
.overlay-select {
  background: var(--color-surface-muted);
  box-sizing: border-box;
  cursor: pointer;
  font-size: 12px;
  margin-bottom: 8px;
  min-height: 0;
  padding: 6px 9px;
  width: 100%;
}
```

Replace with (note: `border` is already set by the global `select` rule, but we set `border-radius` explicitly to override the global 6px with 7px):
```css
.overlay-select {
  background: var(--color-surface-muted);
  border: 1px solid var(--color-border);
  border-radius: 7px;
  box-sizing: border-box;
  cursor: pointer;
  font-size: 11px;
  margin-bottom: 8px;
  min-height: 0;
  padding: 5px 9px;
  width: 100%;
}
```

- [ ] **Step 2: Update `.overlay-dropzone`**

Current:
```css
.overlay-dropzone {
  align-items: center;
  background: var(--color-surface-muted);
  border: 1.5px dashed var(--color-border);
  border-radius: 8px;
  display: flex;
  flex-direction: column;
  gap: 5px;
  justify-content: center;
  min-height: 76px;
  padding: 10px 8px;
  text-align: center;
  transition: border-color 160ms ease, background 160ms ease;
}
```

Replace with:
```css
.overlay-dropzone {
  align-items: center;
  background: var(--color-surface-muted);
  border: 1.5px dashed var(--color-border);
  border-radius: 10px;
  display: flex;
  flex-direction: column;
  gap: 5px;
  justify-content: center;
  min-height: 76px;
  padding: 16px 8px;
  text-align: center;
  transition: border-color 160ms ease, background 160ms ease, color 160ms ease;
}
```

- [ ] **Step 3: Update `.overlay-dropzone-active` and add active label rule**

Current active state:
```css
.overlay-dropzone.overlay-dropzone-active {
  background: var(--color-accent-bg-strong);
  border-color: var(--color-accent);
  border-style: solid;
}
```

Replace with:
```css
.overlay-dropzone.overlay-dropzone-active {
  background: var(--color-accent-bg);
  border-color: var(--color-accent);
  border-style: solid;
  border-width: 2px;
}

.overlay-dropzone.overlay-dropzone-active .overlay-drop-label {
  color: var(--color-accent);
  font-weight: 700;
}
```

- [ ] **Step 4: Update dropzone label text in JSX for drag-over state**

In `DropPanel` in `src/App.tsx`, find:
```tsx
<div className={`overlay-dropzone ${dragOver ? 'overlay-dropzone-active' : ''}`}>
  <span className="overlay-drop-icon">📂</span>
  <span className="overlay-drop-label">{t(locale, 'dropTitle')}</span>
</div>
```

Replace with:
```tsx
<div className={`overlay-dropzone ${dragOver ? 'overlay-dropzone-active' : ''}`}>
  <span className="overlay-drop-icon">📂</span>
  <span className="overlay-drop-label">
    {dragOver ? (locale === 'en-US' ? 'Release to send' : '松手即发送') : t(locale, 'dropTitle')}
  </span>
</div>
```

- [ ] **Step 5: Commit**

```bash
git add src/App.css src/App.tsx
git commit -m "feat: refine select, dropzone styling and active label text"
```

---

## Task 6: Visual Verification

- [ ] **Step 1: Run the dev server**

```bash
pnpm tauri dev
```

- [ ] **Step 2: Verify collapsed handle**

Move the handle to each screen edge. For each edge check:
- Handle is 20×52px (or 52×20 for top/bottom)
- Visible corners are rounded (10px radius), screen-edge side is flat
- Chevron arrow points away from the screen edge
- Breathing animation is visible
- Shadow visible on the inward side (no shadow on screen-edge side)
- Hover: brightness increases, animation pauses

- [ ] **Step 3: Verify expanded panel**

Hover the handle to expand the panel. Check:
- Panel card has 12px border-radius with no border
- Refined shadow (deeper, softer than before)
- Device dropdown: surface-muted background, 7px radius
- Dropzone: 10px radius, more padding, dashed border

- [ ] **Step 4: Verify drag-over state**

Drag a file onto the panel. Check:
- Dropzone border changes from dashed to solid accent color
- Dropzone background changes to `accent-bg`
- Label text changes to "松手即发送" / "Release to send"
- Label color becomes accent color
- Panel card gets faint outer glow ring

- [ ] **Step 5: Verify light and dark mode**

Toggle system appearance between light and dark. Check:
- All colors adapt via CSS variables (no hardcoded colors)
- Shadows are appropriate for each mode

- [ ] **Step 6: Final commit (if any touch-up edits were needed)**

```bash
git add src/App.css src/App.tsx
git commit -m "fix: overlay ui touch-ups from visual review"
```

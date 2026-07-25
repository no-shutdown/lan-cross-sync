export const DROP_HANDLE_LABEL = 'drop-handle'
export const DROP_PANEL_LABEL = 'drop-panel'

export const OVERLAY_HANDLE_LENGTH = 52
export const OVERLAY_HANDLE_THICKNESS = 20

// Derived aliases for convenience
export const OVERLAY_HANDLE_W = OVERLAY_HANDLE_THICKNESS
export const OVERLAY_HANDLE_H = OVERLAY_HANDLE_LENGTH
export const OVERLAY_PANEL_W = 248
export const OVERLAY_PANEL_H = 190
export const OVERLAY_EDGE_MARGIN = 6

export const OVERLAY_EVENT_PANEL_ENTER = 'overlay:panel-enter'
export const OVERLAY_EVENT_PANEL_LEAVE = 'overlay:panel-leave'
export const OVERLAY_EVENT_PANEL_CLOSE = 'overlay:panel-close'
export const OVERLAY_EVENT_DRAG_ENTER = 'overlay:drag-enter'
export const OVERLAY_EVENT_DRAG_STOP = 'overlay:drag-stop'
export const OVERLAY_EVENT_DROP_PATHS = 'overlay:drop-paths'
export const OVERLAY_EVENT_DROP_COMPLETE = 'overlay:drop-complete'

export type OverlayEdge = 'left' | 'right' | 'top' | 'bottom'

export type OverlayDropPayload = {
  paths: string[]
}

export type OverlayBounds = {
  left: number
  top: number
  right: number
  bottom: number
}

export type OverlayHandlePlacement = {
  x: number
  y: number
  edge: OverlayEdge
  width: number
  height: number
}

function clamp(value: number, min: number, max: number) {
  if (max < min) return min
  return Math.min(Math.max(value, min), max)
}

export function defaultOverlayBounds(): OverlayBounds {
  const screen = window.screen as Screen & { availLeft?: number; availTop?: number }
  const left = screen.availLeft ?? 0
  const top = screen.availTop ?? 0
  return {
    left,
    top,
    right: left + screen.availWidth,
    bottom: top + screen.availHeight,
  }
}

export function handleSizeForEdge(edge: OverlayEdge) {
  if (edge === 'left' || edge === 'right') {
    return { width: OVERLAY_HANDLE_THICKNESS, height: OVERLAY_HANDLE_LENGTH }
  }

  return { width: OVERLAY_HANDLE_LENGTH, height: OVERLAY_HANDLE_THICKNESS }
}

export function nearestHandlePlacement(
  position: { x: number; y: number },
  bounds = defaultOverlayBounds(),
  currentSize = handleSizeForEdge('bottom'),
): OverlayHandlePlacement {
  const centerX = position.x + currentSize.width / 2
  const centerY = position.y + currentSize.height / 2
  const distances: Record<OverlayEdge, number> = {
    left: Math.abs(centerX - bounds.left),
    right: Math.abs(bounds.right - centerX),
    top: Math.abs(centerY - bounds.top),
    bottom: Math.abs(bounds.bottom - centerY),
  }
  const edge = (Object.entries(distances) as Array<[OverlayEdge, number]>)
    .reduce((nearest, current) => current[1] < nearest[1] ? current : nearest)[0]
  const size = handleSizeForEdge(edge)

  if (edge === 'left' || edge === 'right') {
    return {
      edge,
      width: size.width,
      height: size.height,
      x: edge === 'left' ? bounds.left : bounds.right - size.width,
      y: clamp(centerY - size.height / 2, bounds.top + OVERLAY_EDGE_MARGIN, bounds.bottom - size.height - OVERLAY_EDGE_MARGIN),
    }
  }

  return {
    edge,
    width: size.width,
    height: size.height,
    x: clamp(centerX - size.width / 2, bounds.left + OVERLAY_EDGE_MARGIN, bounds.right - size.width - OVERLAY_EDGE_MARGIN),
    y: edge === 'top' ? bounds.top : bounds.bottom - size.height,
  }
}

export function panelPositionFromHandle(
  position: { x: number; y: number },
  edge: OverlayEdge = 'right',
) {
  const size = handleSizeForEdge(edge)

  if (edge === 'left') {
    return {
      x: position.x,
      y: position.y + size.height / 2 - OVERLAY_PANEL_H / 2,
    }
  }

  if (edge === 'top') {
    return {
      x: position.x + size.width / 2 - OVERLAY_PANEL_W / 2,
      y: position.y,
    }
  }

  if (edge === 'bottom') {
    return {
      x: position.x + size.width / 2 - OVERLAY_PANEL_W / 2,
      y: position.y + size.height - OVERLAY_PANEL_H,
    }
  }

  return {
    x: position.x + size.width - OVERLAY_PANEL_W,
    y: position.y + size.height / 2 - OVERLAY_PANEL_H / 2,
  }
}

export function clampPanelPosition(
  position: { x: number; y: number },
  bounds = defaultOverlayBounds(),
) {
  return {
    x: clamp(position.x, bounds.left + OVERLAY_EDGE_MARGIN, bounds.right - OVERLAY_PANEL_W - OVERLAY_EDGE_MARGIN),
    y: clamp(position.y, bounds.top + OVERLAY_EDGE_MARGIN, bounds.bottom - OVERLAY_PANEL_H - OVERLAY_EDGE_MARGIN),
  }
}

export function handlePositionFromPanel(position: { x: number; y: number }) {
  return {
    x: position.x + OVERLAY_PANEL_W / 2 - OVERLAY_HANDLE_W / 2,
    y: position.y + OVERLAY_PANEL_H / 2 - OVERLAY_HANDLE_H / 2,
  }
}

export const DROP_HANDLE_LABEL = 'drop-handle'
export const DROP_PANEL_LABEL = 'drop-panel'

export const OVERLAY_HANDLE_W = 72
export const OVERLAY_HANDLE_H = 72
export const OVERLAY_PANEL_W = 248
export const OVERLAY_PANEL_H = 190

export const OVERLAY_EVENT_PANEL_ENTER = 'overlay:panel-enter'
export const OVERLAY_EVENT_PANEL_LEAVE = 'overlay:panel-leave'
export const OVERLAY_EVENT_PANEL_CLOSE = 'overlay:panel-close'
export const OVERLAY_EVENT_DRAG_ENTER = 'overlay:drag-enter'
export const OVERLAY_EVENT_DRAG_STOP = 'overlay:drag-stop'
export const OVERLAY_EVENT_DROP_PATHS = 'overlay:drop-paths'
export const OVERLAY_EVENT_DROP_COMPLETE = 'overlay:drop-complete'

export type OverlayDropPayload = {
  paths: string[]
}

export function panelPositionFromHandle(position: { x: number; y: number }) {
  return {
    x: position.x + OVERLAY_HANDLE_W - OVERLAY_PANEL_W,
    y: position.y + OVERLAY_HANDLE_H - OVERLAY_PANEL_H,
  }
}

export function handlePositionFromPanel(position: { x: number; y: number }) {
  return {
    x: position.x + OVERLAY_PANEL_W - OVERLAY_HANDLE_W,
    y: position.y + OVERLAY_PANEL_H - OVERLAY_HANDLE_H,
  }
}

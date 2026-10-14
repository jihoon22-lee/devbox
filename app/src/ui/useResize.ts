import { type PointerEvent as ReactPointerEvent, useEffect, useRef } from "react";

export interface ResizePoint {
  x: number;
  y: number;
}

/** Freezes the callback at pointerdown so a rerender cannot change the drag origin. */
export function useResize(onMove: (start: ResizePoint, current: ResizePoint) => void) {
  const cleanup = useRef<(() => void) | null>(null);
  useEffect(() => () => cleanup.current?.(), []);
  return (event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    cleanup.current?.();
    event.preventDefault();
    const target = event.currentTarget;
    const pointerId = event.pointerId;
    const start = { x: event.clientX, y: event.clientY };
    target.setPointerCapture?.(pointerId);
    const move = (next: PointerEvent) => {
      if (next.pointerId === pointerId) onMove(start, { x: next.clientX, y: next.clientY });
    };
    const stop = (next?: PointerEvent) => {
      if (next && next.pointerId !== pointerId) return;
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      window.removeEventListener("pointercancel", stop);
      target.removeEventListener("lostpointercapture", stop);
      if (target.hasPointerCapture?.(pointerId)) target.releasePointerCapture?.(pointerId);
      cleanup.current = null;
    };
    cleanup.current = stop;
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
    window.addEventListener("pointercancel", stop);
    target.addEventListener("lostpointercapture", stop);
  };
}

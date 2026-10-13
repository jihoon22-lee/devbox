import { type RefObject, useEffect } from "react";
import { autoCollapse, useLayout } from "./layout";

export function useAutoCollapse(frame: RefObject<HTMLElement | null>) {
  useEffect(() => {
    let previous: ReturnType<typeof autoCollapse> | undefined;
    const update = (width: number, height: number) => {
      const next = autoCollapse(width, height);
      if (next.list && !previous?.list) useLayout.setState({ listCollapsed: true });
      if (next.bottom && !previous?.bottom) useLayout.setState({ bottomOpen: false });
      previous = next;
    };
    const viewport = () => update(window.innerWidth, window.innerHeight);
    update(frame.current?.clientWidth || window.innerWidth, frame.current?.clientHeight || window.innerHeight);
    // Observe the whole frame, never the content shortened by the bottom panel.
    const observer =
      typeof ResizeObserver === "undefined"
        ? undefined
        : new ResizeObserver((entries) => {
            const rect = entries[0]?.contentRect;
            if (rect && rect.width > 0 && rect.height > 0) update(rect.width, rect.height);
          });
    if (frame.current) observer?.observe(frame.current);
    if (!observer) window.addEventListener("resize", viewport);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", viewport);
    };
  }, [frame]);
}

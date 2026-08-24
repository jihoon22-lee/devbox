import {
  useCallback,
  useMemo,
  useState,
  type HTMLAttributes,
  type KeyboardEvent,
  type MouseEvent,
} from "react";
import type { Point } from "./position";

interface OpenState {
  anchor: Point;
  restoreFocusTo: HTMLElement;
}

export interface UseContextMenuOptions {
  disabled?: boolean;
  /** 앱이 row selection 같은 고유 상태를 menu open 전에 동기화할 때 사용한다. */
  onBeforeOpen?: (reason: "pointer" | "keyboard", target: HTMLElement) => void;
}

export interface ContextMenuTriggerProps
  extends Pick<
    HTMLAttributes<HTMLElement>,
    "aria-haspopup" | "aria-expanded" | "onContextMenu" | "onKeyDown"
  > {}

export interface ContextMenuController {
  open: boolean;
  anchor: Point | null;
  restoreFocusTo: HTMLElement | null;
  triggerProps: ContextMenuTriggerProps;
  openAt: (anchor: Point, restoreFocusTo: HTMLElement) => void;
  close: () => void;
}

function keyboardAnchor(target: HTMLElement): Point {
  const rect = target.getBoundingClientRect();
  return {
    x: rect.left + Math.min(24, Math.max(0, rect.width / 2)),
    y: rect.bottom,
  };
}

function isMenuKey(event: KeyboardEvent<HTMLElement>): boolean {
  return event.key === "ContextMenu" || event.code === "ContextMenu";
}

function isComposing(event: KeyboardEvent<HTMLElement>): boolean {
  return event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229;
}

/**
 * 동일 trigger에서 pointer contextmenu와 Shift+F10/Menu key를 한 경로로 연다.
 * 다른 key/clipboard/IME event는 preventDefault 하지 않는다.
 */
export function useContextMenu(options: UseContextMenuOptions = {}): ContextMenuController {
  const [state, setState] = useState<OpenState | null>(null);

  const openAt = useCallback((anchor: Point, restoreFocusTo: HTMLElement) => {
    setState({ anchor, restoreFocusTo });
  }, []);
  const close = useCallback(() => setState(null), []);

  const onContextMenu = useCallback(
    (event: MouseEvent<HTMLElement>) => {
      if (options.disabled) return;
      event.preventDefault();
      options.onBeforeOpen?.("pointer", event.currentTarget);
      openAt(
        { x: event.clientX, y: event.clientY },
        event.currentTarget,
      );
    },
    [openAt, options.disabled, options.onBeforeOpen],
  );

  const onKeyDown = useCallback(
    (event: KeyboardEvent<HTMLElement>) => {
      if (
        options.disabled ||
        isComposing(event) ||
        !(isMenuKey(event) || (event.shiftKey && event.key === "F10"))
      ) {
        return;
      }
      event.preventDefault();
      options.onBeforeOpen?.("keyboard", event.currentTarget);
      openAt(keyboardAnchor(event.currentTarget), event.currentTarget);
    },
    [openAt, options.disabled, options.onBeforeOpen],
  );

  const triggerProps = useMemo<ContextMenuTriggerProps>(
    () => ({
      "aria-haspopup": "menu",
      "aria-expanded": state !== null,
      onContextMenu,
      onKeyDown,
    }),
    [onContextMenu, onKeyDown, state],
  );

  return {
    open: state !== null,
    anchor: state?.anchor ?? null,
    restoreFocusTo: state?.restoreFocusTo ?? null,
    triggerProps,
    openAt,
    close,
  };
}

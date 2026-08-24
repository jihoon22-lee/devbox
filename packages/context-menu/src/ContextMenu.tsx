import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { placeRootMenu, placeSubmenu, type MenuPlacement, type Point } from "./position";
import type { ContextMenuEntry, ContextMenuSubmenu } from "./types";
import "./styles.css";

export interface ContextMenuProps {
  open: boolean;
  anchor: Point | null;
  items: readonly ContextMenuEntry[];
  onSelect: (id: string) => void;
  onClose: () => void;
  restoreFocusTo?: HTMLElement | null;
  ariaLabel?: string;
  className?: string;
}

interface MenuLevelProps {
  items: readonly ContextMenuEntry[];
  onSelect: (id: string) => void;
  onClose: () => void;
  rootAnchor?: Point;
  parentElement?: HTMLElement | null;
  autoFocus: boolean;
  ariaLabel: string;
  onBack?: () => void;
  rootRef?: RefObject<HTMLDivElement | null>;
  className?: string;
}

function interactive(entry: ContextMenuEntry): entry is Exclude<ContextMenuEntry, { type: "separator" }> {
  return entry.type !== "separator";
}

function enabled(
  entry: ContextMenuEntry,
): entry is Exclude<ContextMenuEntry, { type: "separator" }> {
  return interactive(entry) && !entry.disabled;
}

function firstEnabled(items: readonly ContextMenuEntry[]): number {
  return items.findIndex(enabled);
}

function lastEnabled(items: readonly ContextMenuEntry[]): number {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    if (enabled(items[index])) return index;
  }
  return -1;
}

function samePlacement(left: MenuPlacement | null, right: MenuPlacement): boolean {
  return (
    left?.x === right.x &&
    left.y === right.y &&
    left.horizontal === right.horizontal &&
    left.vertical === right.vertical
  );
}

function useMenuPlacement(
  menuRef: RefObject<HTMLDivElement | null>,
  rootAnchor?: Point,
  parentElement?: HTMLElement | null,
): MenuPlacement | null {
  const [placement, setPlacement] = useState<MenuPlacement | null>(null);
  const update = useCallback(() => {
    const menu = menuRef.current;
    if (!menu || typeof window === "undefined") return;
    const menuRect = menu.getBoundingClientRect();
    const viewport = { width: window.innerWidth, height: window.innerHeight };
    const next = rootAnchor
      ? placeRootMenu(rootAnchor, menuRect, viewport)
      : parentElement
        ? placeSubmenu(parentElement.getBoundingClientRect(), menuRect, viewport)
        : null;
    if (next) setPlacement((current) => (samePlacement(current, next) ? current : next));
  }, [menuRef, parentElement, rootAnchor]);

  useLayoutEffect(update, [update]);
  useEffect(() => {
    if (typeof window === "undefined") return;
    window.addEventListener("resize", update);
    return () => window.removeEventListener("resize", update);
  }, [update]);

  return placement;
}

function MenuLevel({
  items,
  onSelect,
  onClose,
  rootAnchor,
  parentElement,
  autoFocus,
  ariaLabel,
  onBack,
  rootRef,
  className,
}: MenuLevelProps) {
  const localRef = useRef<HTMLDivElement>(null);
  const menuRef = rootRef ?? localRef;
  const buttonRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const previousAutoFocus = useRef(false);
  const [activeIndex, setActiveIndex] = useState(() =>
    autoFocus ? firstEnabled(items) : -1,
  );
  const [openSubmenuId, setOpenSubmenuId] = useState<string | null>(null);
  const [keyboardSubmenuId, setKeyboardSubmenuId] = useState<string | null>(null);
  const placement = useMenuPlacement(menuRef, rootAnchor, parentElement);

  const focusIndex = useCallback((index: number) => {
    if (index < 0) return;
    setActiveIndex(index);
    buttonRefs.current[index]?.focus({ preventScroll: true });
  }, []);

  useEffect(() => {
    if (activeIndex >= 0 && !enabled(items[activeIndex])) {
      const next = autoFocus ? firstEnabled(items) : -1;
      if (autoFocus && next >= 0) focusIndex(next);
      else setActiveIndex(next);
    } else if (autoFocus && activeIndex < 0) {
      focusIndex(firstEnabled(items));
    }
    if (
      openSubmenuId &&
      !items.some(
        (entry) => entry.type === "submenu" && entry.id === openSubmenuId && !entry.disabled,
      )
    ) {
      setOpenSubmenuId(null);
      setKeyboardSubmenuId(null);
    }
  }, [activeIndex, autoFocus, focusIndex, items, openSubmenuId]);

  useLayoutEffect(() => {
    if (autoFocus && !previousAutoFocus.current) focusIndex(firstEnabled(items));
    previousAutoFocus.current = autoFocus;
  }, [autoFocus, focusIndex, items]);

  const move = (delta: 1 | -1) => {
    if (items.length === 0) return;
    const start = activeIndex >= 0 ? activeIndex : delta > 0 ? -1 : 0;
    for (let offset = 1; offset <= items.length; offset += 1) {
      const index = (start + delta * offset + items.length) % items.length;
      if (enabled(items[index])) {
        setOpenSubmenuId(null);
        setKeyboardSubmenuId(null);
        focusIndex(index);
        return;
      }
    }
  };

  const selectAction = (id: string) => {
    try {
      onSelect(id);
    } finally {
      onClose();
    }
  };

  const openSubmenu = (entry: ContextMenuSubmenu, index: number) => {
    if (entry.disabled) return;
    focusIndex(index);
    setOpenSubmenuId(entry.id);
    setKeyboardSubmenuId(entry.id);
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const entry = items[activeIndex];
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        event.stopPropagation();
        move(1);
        break;
      case "ArrowUp":
        event.preventDefault();
        event.stopPropagation();
        move(-1);
        break;
      case "Home":
        event.preventDefault();
        event.stopPropagation();
        setOpenSubmenuId(null);
        setKeyboardSubmenuId(null);
        focusIndex(firstEnabled(items));
        break;
      case "End":
        event.preventDefault();
        event.stopPropagation();
        setOpenSubmenuId(null);
        setKeyboardSubmenuId(null);
        focusIndex(lastEnabled(items));
        break;
      case "Tab":
        event.preventDefault();
        event.stopPropagation();
        move(event.shiftKey ? -1 : 1);
        break;
      case "ArrowRight":
        if (entry?.type === "submenu" && !entry.disabled) {
          event.preventDefault();
          event.stopPropagation();
          openSubmenu(entry, activeIndex);
        }
        break;
      case "ArrowLeft":
        if (onBack) {
          event.preventDefault();
          event.stopPropagation();
          onBack();
        }
        break;
      case "Enter":
      case " ":
        if (entry && enabled(entry)) {
          event.preventDefault();
          event.stopPropagation();
          if (entry.type === "submenu") openSubmenu(entry, activeIndex);
          else selectAction(entry.id);
        }
        break;
      case "Escape":
        event.preventDefault();
        event.stopPropagation();
        onClose();
        break;
    }
  };

  const style: CSSProperties = {
    left: placement?.x ?? rootAnchor?.x ?? parentElement?.getBoundingClientRect().right ?? 0,
    top: placement?.y ?? rootAnchor?.y ?? parentElement?.getBoundingClientRect().top ?? 0,
    visibility: placement ? "visible" : "hidden",
  };

  return (
    <div
      ref={menuRef}
      role="menu"
      aria-label={ariaLabel}
      className={["db-context-menu", parentElement ? "db-context-submenu" : "", className]
        .filter(Boolean)
        .join(" ")}
      style={style}
      data-horizontal={placement?.horizontal}
      data-vertical={placement?.vertical}
      onKeyDown={handleKeyDown}
      onContextMenu={(event) => event.preventDefault()}
    >
      {items.map((entry, index) => {
        if (entry.type === "separator") {
          return (
            <div
              key={entry.id ?? `separator-${index}`}
              role="separator"
              className="db-context-menu-separator"
            />
          );
        }
        const isActive = activeIndex === index;
        const hasSubmenu = entry.type === "submenu";
        return (
          <div key={entry.id} className="db-context-menu-entry">
            <button
              ref={(node) => {
                buttonRefs.current[index] = node;
              }}
              type="button"
              role="menuitem"
              aria-label={entry.label}
              tabIndex={isActive ? 0 : -1}
              aria-disabled={entry.disabled || undefined}
              aria-haspopup={hasSubmenu ? "menu" : undefined}
              aria-expanded={hasSubmenu ? openSubmenuId === entry.id : undefined}
              className={[
                "db-context-menu-item",
                isActive ? "active" : "",
                entry.disabled ? "disabled" : "",
                entry.danger ? "danger" : "",
              ]
                .filter(Boolean)
                .join(" ")}
              onMouseEnter={() => {
                if (!enabled(entry)) return;
                focusIndex(index);
                setOpenSubmenuId(
                  entry.type === "submenu" ? entry.id : null,
                );
                setKeyboardSubmenuId(null);
              }}
              onClick={() => {
                if (!enabled(entry)) return;
                if (entry.type === "submenu") openSubmenu(entry, index);
                else selectAction(entry.id);
              }}
            >
              <span className="db-context-menu-label">{entry.label}</span>
              {entry.type === "item" && entry.shortcut ? (
                <span className="db-context-menu-shortcut">{entry.shortcut}</span>
              ) : null}
              {hasSubmenu ? <span className="db-context-menu-arrow" aria-hidden="true">›</span> : null}
            </button>
            {hasSubmenu && openSubmenuId === entry.id && !entry.disabled ? (
              <MenuLevel
                items={entry.items}
                onSelect={onSelect}
                onClose={onClose}
                parentElement={buttonRefs.current[index]}
                autoFocus={keyboardSubmenuId === entry.id}
                ariaLabel={entry.label}
                onBack={() => {
                  setOpenSubmenuId(null);
                  setKeyboardSubmenuId(null);
                  focusIndex(index);
                }}
              />
            ) : null}
          </div>
        );
      })}
    </div>
  );
}

export function ContextMenu({
  open,
  anchor,
  items,
  onSelect,
  onClose,
  restoreFocusTo,
  ariaLabel = "Context menu",
  className,
}: ContextMenuProps) {
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open || typeof document === "undefined") return;
    const target = restoreFocusTo ?? (document.activeElement as HTMLElement | null);
    const menu = rootRef.current;
    return () => {
      const active = document.activeElement;
      if (
        target?.isConnected &&
        (active == null || active === document.body || (menu != null && menu.contains(active)))
      ) {
        target.focus({ preventScroll: true });
      }
    };
  }, [open, restoreFocusTo]);

  useEffect(() => {
    if (!open || typeof document === "undefined") return;
    const handlePointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && !rootRef.current?.contains(event.target)) onClose();
    };
    document.addEventListener("pointerdown", handlePointerDown, true);
    return () => document.removeEventListener("pointerdown", handlePointerDown, true);
  }, [onClose, open]);

  useEffect(() => {
    if (!open || typeof document === "undefined" || typeof window === "undefined") return;
    const handleScroll = (event: Event) => {
      if (event.target instanceof Node && rootRef.current?.contains(event.target)) return;
      onClose();
    };
    document.addEventListener("scroll", handleScroll, true);
    window.addEventListener("scroll", handleScroll, true);
    return () => {
      document.removeEventListener("scroll", handleScroll, true);
      window.removeEventListener("scroll", handleScroll, true);
    };
  }, [onClose, open]);

  if (!open || !anchor || typeof document === "undefined") return null;
  return createPortal(
    <MenuLevel
      items={items}
      onSelect={onSelect}
      onClose={onClose}
      rootAnchor={anchor}
      autoFocus
      ariaLabel={ariaLabel}
      rootRef={rootRef}
      className={className}
    />,
    document.body,
  );
}

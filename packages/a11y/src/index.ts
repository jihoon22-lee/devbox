export interface CompositionAwareKeyboardEvent {
  key: string;
  keyCode?: number;
  isComposing?: boolean;
  nativeEvent?: {
    keyCode?: number;
    isComposing?: boolean;
  };
}

export interface DialogKeyboardEvent extends CompositionAwareKeyboardEvent {
  shiftKey: boolean;
  preventDefault(): void;
  stopPropagation(): void;
}

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled]):not([type='hidden'])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "summary",
  "[contenteditable='true']",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

export function isImeComposing(event: CompositionAwareKeyboardEvent): boolean {
  return Boolean(
    event.isComposing
      || event.nativeEvent?.isComposing
      || event.keyCode === 229
      || event.nativeEvent?.keyCode === 229,
  );
}

export function isKeyboardActivation(event: CompositionAwareKeyboardEvent): boolean {
  return !isImeComposing(event) && (event.key === "Enter" || event.key === " ");
}

export function focusableElements(root: ParentNode): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter((element) => {
    if (
      element.hidden
      || element.matches(":disabled")
      || element.getAttribute("aria-hidden") === "true"
    ) return false;
    return !element.closest("[hidden], [aria-hidden='true'], [inert]");
  });
}

export function focusFirst(root: ParentNode): HTMLElement | null {
  const target = focusableElements(root)[0] ?? null;
  target?.focus({ preventScroll: true });
  return target;
}

export function trapDialogKeyDown(
  event: DialogKeyboardEvent,
  root: ParentNode,
  onEscape?: () => void,
): boolean {
  if (isImeComposing(event)) return false;

  if (event.key === "Escape" && onEscape) {
    event.preventDefault();
    event.stopPropagation();
    onEscape();
    return true;
  }

  if (event.key !== "Tab") return false;
  const elements = focusableElements(root);
  if (elements.length === 0) {
    event.preventDefault();
    event.stopPropagation();
    return true;
  }

  const first = elements[0];
  const last = elements[elements.length - 1];
  const active = root.ownerDocument?.activeElement;
  if (event.shiftKey && (active === first || !root.contains(active ?? null))) {
    event.preventDefault();
    event.stopPropagation();
    last.focus({ preventScroll: true });
    return true;
  }
  if (!event.shiftKey && (active === last || !root.contains(active ?? null))) {
    event.preventDefault();
    event.stopPropagation();
    first.focus({ preventScroll: true });
    return true;
  }
  return false;
}

export function restoreFocus(target: HTMLElement | null | undefined): boolean {
  if (!target?.isConnected) return false;
  target.focus({ preventScroll: true });
  return true;
}

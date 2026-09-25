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
  "[tabindex]",
].join(",");

export function isImeComposing(event: CompositionAwareKeyboardEvent): boolean {
  return Boolean(
    event.isComposing || event.nativeEvent?.isComposing || event.keyCode === 229 || event.nativeEvent?.keyCode === 229,
  );
}

export function isKeyboardActivation(event: CompositionAwareKeyboardEvent): boolean {
  return !isImeComposing(event) && (event.key === "Enter" || event.key === " ");
}

export function focusableElements(root: ParentNode): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter((element) => {
    if (element.hidden || element.matches(":disabled") || element.getAttribute("aria-hidden") === "true") return false;
    if (element.closest("[hidden], [aria-hidden='true'], [inert]")) return false;
    const view = element.ownerDocument.defaultView;
    if (!view) return false;
    const visibility = view.getComputedStyle(element).visibility;
    if (visibility === "hidden" || visibility === "collapse") return false;
    for (let parent: HTMLElement | null = element; parent; parent = parent.parentElement) {
      const style = view.getComputedStyle(parent);
      if (style.display === "none" || style.contentVisibility === "hidden") return false;
      if (parent.tagName === "DETAILS" && !parent.hasAttribute("open")) {
        const summary = Array.from(parent.children).find((child) => child.tagName === "SUMMARY");
        if (!summary?.contains(element)) return false;
      }
    }
    return true;
  });
}

/** Sequential keyboard navigation excludes negative tabindex and follows positive tabindex order. */
export function tabbableElements(root: ParentNode): HTMLElement[] {
  const tabIndex = (element: HTMLElement) =>
    !element.hasAttribute("tabindex") && element.matches("[contenteditable='true']") ? 0 : element.tabIndex;
  const eligible = focusableElements(root).filter((element) => tabIndex(element) >= 0);
  return eligible
    .filter((element) => {
      if (!element.matches("input[type='radio']") || !(element as HTMLInputElement).name) return true;
      const radio = element as HTMLInputElement;
      const group = focusableElements(element.ownerDocument).filter(
        (candidate) =>
          candidate.matches("input[type='radio']") &&
          (candidate as HTMLInputElement).name === radio.name &&
          (candidate as HTMLInputElement).form === radio.form,
      ) as HTMLInputElement[];
      return (group.find((candidate) => candidate.checked) ?? group[0]) === radio;
    })
    .sort((a, b) => (tabIndex(a) > 0 ? tabIndex(a) : Infinity) - (tabIndex(b) > 0 ? tabIndex(b) : Infinity));
}

export function focusFirst(root: ParentNode): HTMLElement | null {
  const target = tabbableElements(root)[0] ?? null;
  target?.focus({ preventScroll: true });
  return target;
}

export function trapDialogKeyDown(event: DialogKeyboardEvent, root: ParentNode, onEscape?: () => void): boolean {
  if (isImeComposing(event)) return false;

  if (event.key === "Escape" && onEscape) {
    event.preventDefault();
    event.stopPropagation();
    onEscape();
    return true;
  }

  if (event.key !== "Tab") return false;
  const elements = tabbableElements(root);
  if (elements.length === 0) {
    event.preventDefault();
    event.stopPropagation();
    return true;
  }

  const first = elements[0];
  const last = elements[elements.length - 1];
  const active = root.ownerDocument?.activeElement;
  if (event.shiftKey && (active === first || !elements.includes(active as HTMLElement))) {
    event.preventDefault();
    event.stopPropagation();
    last.focus({ preventScroll: true });
    return true;
  }
  if (!event.shiftKey && (active === last || !elements.includes(active as HTMLElement))) {
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

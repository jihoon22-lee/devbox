import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import { messageOf } from "@devbox/hooks";
interface Offer {
  id: number;
  label: string;
  undo: (() => Promise<void>) | null;
  opener: HTMLElement | null;
  expiresAt: number;
  busy: boolean;
}
interface Control {
  offer(label: string, undo: () => Promise<void>): void;
  toast: ReactNode;
}
const UndoContext = createContext<Control | null>(null);
function restore(offer: Offer, toast: HTMLDivElement | null): void {
  if (!toast?.contains(document.activeElement)) return;
  const opener = offer.opener;
  if (opener?.isConnected && !opener.closest("[hidden]") && !opener.hasAttribute("disabled")) opener.focus();
  else {
    const main = document.querySelector<HTMLElement>("#product-content, main[tabindex], [role=main]");
    if (main) main.focus();
    else {
      const previous = document.body.getAttribute("tabindex");
      document.body.tabIndex = -1;
      document.body.focus();
      if (previous === null) document.body.removeAttribute("tabindex");
      else document.body.setAttribute("tabindex", previous);
    }
  }
}
export function UndoToast({
  current,
  run,
  toastRef,
}: {
  current: Offer;
  run: () => void;
  toastRef: RefObject<HTMLDivElement | null>;
}) {
  return (
    <div role="status" className="shell-undo" ref={toastRef}>
      {current.label}
      {current.undo && (
        <button type="button" disabled={current.busy} onClick={run}>
          {current.busy ? "되돌리는 중…" : "되돌리기"}
        </button>
      )}
    </div>
  );
}
function useController(): Control {
  const [current, setCurrent] = useState<Offer | null>(null);
  const active = useRef<Offer | null>(null);
  const sequence = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const toastRef = useRef<HTMLDivElement>(null);
  const clear = useCallback((id: number) => {
    if (active.current?.id !== id) return;
    restore(active.current, toastRef.current);
    active.current = null;
    clearTimeout(timer.current);
    setCurrent(null);
  }, []);
  const offer = useCallback(
    (label: string, undo: () => Promise<void>) => {
      let opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      if (active.current && toastRef.current?.contains(opener)) {
        opener = active.current.opener;
        restore(active.current, toastRef.current);
      }
      clearTimeout(timer.current);
      const next = { id: ++sequence.current, label, undo, opener, expiresAt: Date.now() + 8000, busy: false };
      active.current = next;
      setCurrent(next);
      timer.current = setTimeout(() => clear(next.id), 8000);
    },
    [clear],
  );
  useEffect(
    () => () => {
      clearTimeout(timer.current);
      if (active.current) restore(active.current, toastRef.current);
      active.current = null;
    },
    [],
  );
  const run = async () => {
    const selected = active.current;
    if (!selected?.undo || selected.busy) return;
    if (Date.now() >= selected.expiresAt) {
      clear(selected.id);
      return;
    }
    clearTimeout(timer.current);
    const running = { ...selected, busy: true };
    active.current = running;
    setCurrent(running);
    try {
      await selected.undo();
      clear(selected.id);
    } catch (cause) {
      if (active.current?.id !== selected.id) return;
      restore(selected, toastRef.current);
      const failed = { ...selected, label: "되돌리지 못했습니다: " + messageOf(cause), undo: null, busy: false };
      active.current = failed;
      setCurrent(failed);
      timer.current = setTimeout(() => clear(selected.id), 3000);
    }
  };
  return {
    offer,
    toast: current ? (
      <UndoToast
        current={current}
        run={() => {
          void run();
        }}
        toastRef={toastRef}
      />
    ) : null,
  };
}
export function UndoProvider({ children }: { children: ReactNode }) {
  const control = useController();
  return (
    <UndoContext.Provider value={control}>
      {children}
      {control.toast}
    </UndoContext.Provider>
  );
}
export function useUndo(): Control {
  const shared = useContext(UndoContext);
  const local = useController();
  return shared ? { offer: shared.offer, toast: null } : local;
}

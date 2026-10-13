import { CheckCircle2, CircleAlert, Info, X } from "lucide-react";
import { create } from "zustand";
import { Button } from "./Button";
import s from "./Toast.module.css";

export interface ToastInput {
  tone: "info" | "warn" | "danger" | "ok";
  title: string;
  body?: string;
  undo?: () => void;
}
interface Toast extends ToastInput {
  id: number;
  expiresAt: number;
}
interface Toasts {
  items: Toast[];
  push(input: ToastInput): number;
  dismiss(id: number): void;
  undo(id: number): boolean;
  undoLatest(): boolean;
}
let next = 1;
const timers = new Map<number, ReturnType<typeof setTimeout>>();
export const useToasts = create<Toasts>((set, get) => ({
  items: [],
  push: (input) => {
    const id = next++;
    const duration = input.undo ? 10000 : 5000;
    const items = get().items;
    if (items.length >= 5) items.slice(0, items.length - 4).forEach((item) => get().dismiss(item.id));
    timers.set(
      id,
      setTimeout(() => get().dismiss(id), duration),
    );
    set({ items: [...get().items, { ...input, id, expiresAt: Date.now() + duration }] });
    return id;
  },
  dismiss: (id) => {
    clearTimeout(timers.get(id));
    timers.delete(id);
    set({ items: get().items.filter((item) => item.id !== id) });
  },
  undo: (id) => {
    const item = get().items.find((t) => t.id === id);
    if (!item?.undo) return false;
    get().dismiss(id);
    if (Date.now() >= item.expiresAt) return false;
    item.undo();
    return true;
  },
  undoLatest: () => {
    const latest = get().items.findLast((item) => item.undo && Date.now() < item.expiresAt);
    return latest ? get().undo(latest.id) : false;
  },
}));
export function ToastHost() {
  const items = useToasts((s) => s.items);
  const dismiss = useToasts((s) => s.dismiss);
  const undo = useToasts((s) => s.undo);
  return (
    <div className={s.host} role="region" aria-label="알림">
      {items.map((item) => {
        const Icon = item.tone === "ok" ? CheckCircle2 : item.tone === "info" ? Info : CircleAlert;
        return (
          <div key={item.id} className={s.toast} data-tone={item.tone} role="status">
            <Icon className={s.icon} size={16} aria-hidden="true" />
            <div className={s.text}>
              <strong>{item.title}</strong>
              {item.body && <span>{item.body}</span>}
            </div>
            {item.undo && (
              <Button size="sm" variant="ghost" onClick={() => undo(item.id)}>
                되돌리기
              </Button>
            )}
            <Button size="sm" variant="ghost" aria-label={`${item.title} 알림 닫기`} onClick={() => dismiss(item.id)}>
              <X size={14} aria-hidden="true" />
            </Button>
          </div>
        );
      })}
    </div>
  );
}

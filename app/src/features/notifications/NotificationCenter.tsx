import * as D from "@radix-ui/react-dialog";
import { useNavigate } from "@tanstack/react-router";
import { Bell, X } from "lucide-react";
import { useState } from "react";
import { useVisibleClock } from "../../shell/visibility";
import { Button } from "../../ui/Button";
import { runNavigationState } from "../runs/search";
import s from "./NotificationCenter.module.css";
import { safeTarget, useNotifications } from "./store";
export function relativeTime(atMs: number, now: number) {
  const minutes = Math.floor(Math.max(0, now - atMs) / 60_000);
  return minutes === 0
    ? "방금"
    : minutes < 60
      ? `${minutes}분 전`
      : minutes < 1440
        ? `${Math.floor(minutes / 60)}시간 전`
        : `${Math.floor(minutes / 1440)}일 전`;
}
export function NotificationCenter() {
  const [open, setOpen] = useState(false);
  const { items, unread, markRead, markAllRead, clear } = useNotifications();
  const navigate = useNavigate();
  const now = useVisibleClock();
  return (
    <D.Root open={open} onOpenChange={setOpen} modal={false}>
      <D.Trigger asChild>
        <Button variant="ghost" size="sm" aria-label={`알림 ${unread}개`}>
          <Bell size={16} aria-hidden="true" />
          {unread > 0 && <span>{unread}</span>}
        </Button>
      </D.Trigger>
      <D.Portal>
        <D.Content className={s.center} aria-describedby={undefined}>
          <div className={s.header}>
            <D.Title>알림 센터 · 읽지 않음 {unread}개</D.Title>
            <D.Close asChild>
              <Button size="sm" variant="ghost" aria-label="알림 센터 닫기">
                <X size={14} />
              </Button>
            </D.Close>
          </div>
          <div className={s.actions}>
            <Button size="sm" onClick={markAllRead}>
              모두 읽음
            </Button>
            <Button size="sm" onClick={clear}>
              비우기
            </Button>
          </div>
          <div className={s.items}>
            {!items.length && <p>알림이 없습니다</p>}
            {items.map((item) => (
              <button
                key={item.key}
                type="button"
                className={s.item}
                data-read={item.read}
                onClick={() => {
                  markRead(item.key);
                  setOpen(false);
                  const target = safeTarget(item.target);
                  if (target)
                    void navigate({
                      to: target.to,
                      ...(target.to === "/runs" ? { state: runNavigationState() } : {}),
                      search: (previous: Record<string, unknown>) => ({
                        ...(typeof previous.gateway === "string" ? { gateway: previous.gateway } : {}),
                        ...(previous.renderer === "dom" ? { renderer: "dom" } : {}),
                        ...target.search,
                      }),
                    });
                }}
              >
                <strong>{item.title}</strong>
                {item.body && <span>{item.body}</span>}
                <time dateTime={new Date(item.atMs).toISOString()}>{relativeTime(item.atMs, now)}</time>
              </button>
            ))}
          </div>
        </D.Content>
      </D.Portal>
    </D.Root>
  );
}

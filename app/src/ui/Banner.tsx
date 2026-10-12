import type { ReactNode } from "react";
import s from "./Banner.module.css";

export function Banner({
  tone,
  children,
  actions,
}: {
  tone: "info" | "warn" | "danger";
  children: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <div role={tone === "danger" ? "alert" : "status"} className={`${s.banner} ${s[tone]}`}>
      <span className={s.text}>{children}</span>
      {actions && <span className={s.actions}>{actions}</span>}
    </div>
  );
}

import type { ComponentPropsWithRef } from "react";
import s from "./Badge.module.css";

export function Badge({
  tone = "neutral",
  className,
  ...rest
}: ComponentPropsWithRef<"span"> & { tone?: "neutral" | "ok" | "warn" | "danger" | "info" | "progress" }) {
  return <span {...rest} data-tone={tone} className={[s.badge, className].filter(Boolean).join(" ")} />;
}

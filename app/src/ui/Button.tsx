import type { ComponentPropsWithRef } from "react";
import s from "./Button.module.css";

export type ButtonProps = ComponentPropsWithRef<"button"> & {
  variant?: "primary" | "secondary" | "ghost" | "danger";
  size?: "sm" | "md";
};

export function Button({ variant = "secondary", size = "md", className, type = "button", ...rest }: ButtonProps) {
  return (
    <button type={type} className={[s.button, s[variant], s[size], className].filter(Boolean).join(" ")} {...rest} />
  );
}

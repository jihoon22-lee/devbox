import * as D from "@radix-ui/react-dialog";
import { useRef, type ComponentPropsWithoutRef, type ReactNode } from "react";
import s from "./Dialog.module.css";

export function Dialog(props: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  title: string;
  description?: string;
  children?: ReactNode;
  footer?: ReactNode;
  width?: number;
  onOpenAutoFocus?: ComponentPropsWithoutRef<typeof D.Content>["onOpenAutoFocus"];
}) {
  const previousFocus = useRef<HTMLElement | null>(null);
  return (
    <D.Root open={props.open} onOpenChange={props.onOpenChange}>
      <D.Portal>
        <D.Overlay data-dialog-overlay="" className={s.overlay} />
        <D.Content
          className={s.content}
          style={{ width: props.width ?? 480 }}
          onOpenAutoFocus={(event) => {
            previousFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
            props.onOpenAutoFocus?.(event);
          }}
          onCloseAutoFocus={(event) => {
            event.preventDefault();
            if (previousFocus.current?.isConnected) previousFocus.current.focus();
          }}
          {...(props.description ? {} : { "aria-describedby": undefined })}
        >
          <D.Title className={s.title}>{props.title}</D.Title>
          {props.description && <D.Description className={s.description}>{props.description}</D.Description>}
          <div className={s.body}>{props.children}</div>
          {props.footer && <div className={s.footer}>{props.footer}</div>}
        </D.Content>
      </D.Portal>
    </D.Root>
  );
}

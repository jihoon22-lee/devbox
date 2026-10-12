import { useRef } from "react";
import { Button } from "./Button";
import { Dialog } from "./Dialog";

export function ConfirmDialog(props: {
  open: boolean;
  title: string;
  body: string;
  confirmLabel: string;
  tone?: "danger" | "primary";
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const cancel = useRef<HTMLButtonElement>(null);
  return (
    <Dialog
      open={props.open}
      onOpenAutoFocus={(event) => {
        event.preventDefault();
        cancel.current?.focus();
      }}
      onOpenChange={(o) => !o && props.onCancel()}
      title={props.title}
      description={props.body}
      footer={
        <>
          <Button ref={cancel} onClick={props.onCancel}>
            취소
          </Button>
          <Button variant={props.tone === "primary" ? "primary" : "danger"} onClick={props.onConfirm}>
            {props.confirmLabel}
          </Button>
        </>
      }
    />
  );
}

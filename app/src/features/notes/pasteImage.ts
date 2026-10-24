import type { Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { RpcError } from "../../rpc/client";
import type { Attached, AttachImage } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";

/** The daemon's attachment limit (`notes.attach`); larger images are refused before any call. */
export const MAX_IMAGE_BYTES = 2 * 1024 * 1024;

export interface PasteDeps {
  notePath: string;
  attach(params: AttachImage): Promise<Attached>;
  onError(message: string): void;
  /** False once the editor shows another note (the paste finished after a switch). */
  stillCurrent?(): boolean;
}

function base64(bytes: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000)
    binary += String.fromCharCode(...bytes.subarray(i, Math.min(bytes.length, i + 0x8000)));
  return btoa(binary);
}

/** `Blob.arrayBuffer` where available, `FileReader` otherwise (older WebViews, jsdom). */
function bytesOf(blob: Blob): Promise<Uint8Array> {
  if (typeof blob.arrayBuffer === "function") return blob.arrayBuffer().then((buffer) => new Uint8Array(buffer));
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(new Uint8Array(reader.result as ArrayBuffer));
    reader.onerror = () => reject(reader.error);
    reader.readAsArrayBuffer(blob);
  });
}

/** Saves a pasted image with `notes.attach` and writes its Markdown at the cursor (one user edit). */
export async function pasteImage(view: EditorView, file: Blob & { name?: string }, deps: PasteDeps): Promise<void> {
  if (file.size > MAX_IMAGE_BYTES) {
    deps.onError("이미지가 2 MiB를 넘어 붙여 넣지 않았습니다.");
    return;
  }
  const subtype = file.type.split("/")[1]?.replace(/[^a-z0-9]/gi, "") || "png";
  const name = file.name?.trim() || `image.${subtype}`;
  let attached: Attached;
  try {
    const data = base64(await bytesOf(file));
    attached = await deps.attach({ notePath: deps.notePath, name, data });
  } catch (error) {
    deps.onError(
      error instanceof RpcError ? messageFor(error.code, error.detail).title : "이미지를 첨부하지 못했습니다.",
    );
    return;
  }
  if (deps.stillCurrent && !deps.stillCurrent()) {
    deps.onError(`이미지는 ${attached.path}에 저장했지만 다른 노트로 바뀌어 링크를 넣지 않았습니다.`);
    return;
  }
  const { from, to } = view.state.selection.main;
  view.dispatch({
    changes: { from, to, insert: attached.markdown },
    selection: { anchor: from + attached.markdown.length },
    userEvent: "input.paste",
  });
}

/** Paste handler for the note editor: takes the first image of the clipboard, if any. */
export function imagePaste(deps: () => PasteDeps | null): Extension {
  return EditorView.domEventHandlers({
    paste(event, view) {
      const file = [...(event.clipboardData?.files ?? [])].find((item) => item.type.startsWith("image/"));
      const current = deps();
      if (!file || !current || view.state.readOnly) return false;
      event.preventDefault();
      void pasteImage(view, file, current);
      return true;
    },
  });
}

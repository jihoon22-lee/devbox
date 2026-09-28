import { openUrl as nativeOpenUrl } from "@tauri-apps/plugin-opener";
import { isTauri } from "../terminal/lib/isTauri";
/** PR links require HTTPS. Terminal callers retain their validated HTTP links. */
export async function openUrl(value: string, allowHttp = false): Promise<void> {
  const url = new URL(value);
  if (
    (!value.startsWith("https://") && !(allowHttp && value.startsWith("http://"))) ||
    url.username ||
    url.password ||
    /[\u0000-\u001f\u007f]/.test(value)
  )
    throw new Error("열 수 없는 웹 주소입니다.");
  if (isTauri()) await nativeOpenUrl(value);
  else window.open(value, "_blank", "noopener,noreferrer");
}

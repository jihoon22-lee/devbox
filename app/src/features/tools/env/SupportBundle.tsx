import { invoke } from "@tauri-apps/api/core";
import { useRef, useState } from "react";
import type { SupportBundle as Bundle } from "../../../rpc/gen/rpc";
import { useRpcMutation } from "../../../rpc/query";
import { Button } from "../../../ui";
export function SupportBundle() {
  const mutation = useRpcMutation("tools.support_bundle");
  const [bundle, setBundle] = useState<Bundle | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const running = useRef(false);
  const create = async () => {
    if (running.current) return;
    running.current = true;
    setBusy(true);
    setNotice("");
    setBundle(null);
    try {
      setBundle(await mutation.mutateAsync({}));
    } catch {
      setNotice("지원 번들 생성 결과를 확인하지 못했습니다. 저장 공간과 로그·설정 경로를 확인해 주세요.");
    } finally {
      running.current = false;
      setBusy(false);
    }
  };
  const open = async () => {
    if (!bundle) return;
    const parent = bundle.path.slice(0, bundle.path.lastIndexOf("/"));
    try {
      await invoke("open_external", { kind: "explorer", path: parent });
    } catch {
      setNotice("지원 번들 폴더를 열지 못했습니다");
    }
  };
  return (
    <section aria-label="지원 번들">
      <h3>지원 번들</h3>
      <p>
        최근 7일 데몬 로그와 crash 로그, 점검 결과를 로컬 ZIP 파일로 만듭니다. 알려진 비밀과 홈·노트 경로를 가립니다.
        자동으로 전송하지 않습니다.
      </p>
      <Button disabled={busy} onClick={() => void create()}>
        지원 번들
      </Button>
      {busy && <p role="status">지원 번들을 만드는 중…</p>}
      {bundle && (
        <>
          <p>
            <code>{bundle.path}</code> · {bundle.sizeBytes.toLocaleString()}바이트
          </p>
          {bundle.logsOmitted === "secrets_locked" && (
            <p role="status">비밀 저장소가 잠겨 있어 로그를 넣지 않았습니다</p>
          )}
          <Button onClick={() => void open()}>폴더 열기</Button>
        </>
      )}
      {notice && <p role="status">{notice}</p>}
    </section>
  );
}

/**
 * gRPC·MCP 연결 파일(`*.grpc.json`·`*.mcp.json`) 목록과 읽은 파일의 검토 상태. 검토하지 않은 프로젝트 파일은
 * 화면이 연결·호출에 쓰지 않고, [검토하고 사용]을 누르면 읽은 digest로 검토를 기록한 뒤 다시 읽어
 * 그사이 바뀌지 않았을 때만 쓴다(편집기 `confirmReview`와 같은 규칙, 07b 34행).
 */
import { useState } from "react";
import type { CollectionRoot } from "../../../rpc/gen/rpc";
import { messageFor } from "../../../rpc/messages";
import { Button } from "../../../ui";
import s from "./Api.module.css";
import { SendFailure, useApiBackend } from "./backend";
import { useCollections } from "./useCollections";

export interface RootChoice {
  id: string;
  label: string;
  root: CollectionRoot;
}

export interface LoadedFile {
  root: CollectionRoot;
  path: string;
  digest: string;
  reviewed: boolean;
  text: string;
}

export const CHANGED_DURING_REVIEW = "검토하는 동안 파일이 바뀌었습니다. 내용을 확인한 뒤 다시 시도하세요.";

const failureTitle = (error: unknown, fallback: string) => {
  const code = error instanceof SendFailure ? error.code : "internal";
  const detail = error instanceof SendFailure ? error.detail : undefined;
  return messageFor(code as never, detail)?.title ?? fallback;
};

export function useProfileFile() {
  const backend = useApiBackend();
  const [file, setFile] = useState<LoadedFile | null>(null);
  const [notice, setNotice] = useState<string>();
  return {
    file,
    notice,
    setNotice,
    clear: () => setFile(null),
    async load(root: CollectionRoot, path: string): Promise<LoadedFile | null> {
      setNotice(undefined);
      try {
        const read = await backend.read(root, path);
        const loaded = { root, path, digest: read.digest, reviewed: read.reviewed, text: read.text };
        setFile(loaded);
        return loaded;
      } catch (error) {
        setNotice(failureTitle(error, "파일을 읽지 못했습니다."));
        return null;
      }
    },
    /** 검토를 기록하고 다시 읽는다. 그사이 바뀌었으면 검토하지 않은 새 내용으로 두고 `null`. */
    async review(): Promise<LoadedFile | null> {
      if (!file) return null;
      try {
        await backend.review(file.root, file.path, file.digest);
        const again = await backend.read(file.root, file.path);
        if (again.digest !== file.digest) {
          setFile({ ...file, digest: again.digest, text: again.text, reviewed: again.reviewed });
          setNotice(CHANGED_DURING_REVIEW);
          return null;
        }
        const reviewed = { ...file, reviewed: true };
        setFile(reviewed);
        setNotice(undefined);
        return reviewed;
      } catch (error) {
        setNotice(failureTitle(error, "검토를 기록하지 못했습니다."));
        return null;
      }
    },
  };
}

function RootFiles({
  choice,
  suffix,
  onOpen,
}: {
  choice: RootChoice;
  suffix: string;
  onOpen: (root: CollectionRoot, path: string) => void;
}) {
  const list = useCollections(choice.root);
  const items = list.items.filter((item) => item.path.endsWith(suffix)).sort((a, b) => a.path.localeCompare(b.path));
  if (items.length === 0) return null;
  return (
    <div>
      <h4 className={s.muted}>{choice.label}</h4>
      {items.map((item) => (
        <Button key={item.path} size="sm" variant="ghost" onClick={() => onOpen(choice.root, item.path)}>
          {item.path}
        </Button>
      ))}
    </div>
  );
}

/** 루트마다 `suffix`로 끝나는 파일 버튼. */
export function ProfileFiles({
  roots,
  suffix,
  label,
  onOpen,
}: {
  roots: RootChoice[];
  suffix: string;
  label: string;
  onOpen: (root: CollectionRoot, path: string) => void;
}) {
  return (
    <section aria-label={label}>
      {roots.map((choice) => (
        <RootFiles key={choice.id} choice={choice} suffix={suffix} onOpen={onOpen} />
      ))}
    </section>
  );
}

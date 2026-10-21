import { useState } from "react";
import type { LogSource } from "../../rpc/gen/rpc";
import { Button } from "../../ui";
import s from "./LogView.module.css";
import { sourceKey } from "./model";

export function LogSources({
  sources,
  onAdd,
  onRemove,
}: {
  sources: LogSource[];
  onAdd: (source: LogSource) => void;
  onRemove: (source: LogSource) => void;
}) {
  const [adding, setAdding] = useState(false);
  const [kind, setKind] = useState<LogSource["kind"]>("run");
  const [value, setValue] = useState("");
  return (
    <div className={s.sources}>
      {sources.map((source) => (
        <span key={sourceKey(source)} className={s.source}>
          {sourceKey(source)}
          <Button
            size="sm"
            variant="ghost"
            aria-label={`${sourceKey(source)} 소스 제거`}
            onClick={() => onRemove(source)}
          >
            ×
          </Button>
        </span>
      ))}
      <Button size="sm" disabled={sources.length >= 8} onClick={() => setAdding(!adding)}>
        소스 추가
      </Button>
      {adding && (
        <form
          className={s.toolbar}
          onSubmit={(event) => {
            event.preventDefault();
            if (!value.trim() || sources.length >= 8) return;
            onAdd(
              kind === "run"
                ? { kind, runId: value.trim() }
                : kind === "file"
                  ? { kind, path: value.trim() }
                  : { kind, id: value.trim() },
            );
            setValue("");
            setAdding(false);
          }}
        >
          <select
            aria-label="소스 종류"
            value={kind}
            onChange={(event) => setKind(event.target.value as LogSource["kind"])}
          >
            <option value="run">실행</option>
            <option value="file">파일</option>
            <option value="container">컨테이너</option>
          </select>
          <input
            aria-label="소스 경로 또는 ID"
            value={value}
            onChange={(event) => setValue(event.target.value)}
            placeholder={kind === "file" ? "/var/log/파일.log" : "실행 또는 컨테이너 ID"}
          />
          <Button type="submit" size="sm" disabled={!value.trim()}>
            추가
          </Button>
        </form>
      )}
    </div>
  );
}

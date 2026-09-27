import { useEffect, useRef, useState } from "react";
import { useUndo } from "@devbox/product-shell/undo";
import { readImportFiles, type ImportFileFormat as NativeImportFileFormat } from "./api";
import { detectFormat, parseImport, toImportPreview } from "./lib/importers";
export type ImportPreview = ReturnType<typeof toImportPreview>;
interface Props {
  onClose: () => void;
  onApply: (preview: ImportPreview) => Promise<() => Promise<void>>;
}
export function ImportDialog({ onClose, onApply }: Props) {
  const [mode, setMode] = useState<"curl" | "file">("curl");
  const [format, setFormat] = useState<NativeImportFileFormat>("auto");
  const [curl, setCurl] = useState("");
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [includeEnvironments, setIncludeEnvironments] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const panel = useRef<HTMLElement>(null);
  const generation = useRef(0);
  const busyRef = useRef(false);
  const { offer, toast } = useUndo();
  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    panel.current?.querySelector<HTMLElement>("button")?.focus();
    return () => {
      generation.current++;
      if (opener?.isConnected) opener.focus();
    };
  }, []);
  const reset = () => {
    generation.current++;
    setPreview(null);
    setError(null);
    setSelected(new Set());
  };
  const showPreview = (next: ImportPreview) => {
    setPreview(next);
    setSelected(new Set(next.collections.collections.map((item) => item.id)));
  };
  const read = async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    setPreview(null);
    const current = ++generation.current;
    try {
      const files =
        mode === "curl" ? [{ name: "curl", relativePath: "curl", text: curl }] : await readImportFiles(format);
      if (current !== generation.current || !files) return;
      const actual =
        mode === "curl"
          ? "curl"
          : format === "auto"
            ? detectFormat(files[0]?.name ?? "", files[0]?.text ?? "")
            : format;
      if (!actual) throw new Error();
      let sequence = 0;
      showPreview(toImportPreview(parseImport(actual, files), () => `import-${current}-${sequence++}`));
    } catch {
      if (current === generation.current) setError("가져올 수 없는 형식입니다.");
    } finally {
      busyRef.current = false;
      if (current === generation.current) setBusy(false);
    }
  };
  const apply = async () => {
    if (!preview || busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    const current = generation.current;
    const chosen = preview.collections.collections.filter((item) => selected.has(item.id));
    try {
      const undo = await onApply({
        ...preview,
        collections: { ...preview.collections, collections: chosen },
        environments: {
          ...preview.environments,
          environments: includeEnvironments ? preview.environments.environments : [],
        },
      });
      if (current !== generation.current) return;
      offer(`${chosen.length}개를 가져왔습니다.`, undo);
      setPreview(null);
    } catch {
      if (current === generation.current)
        setError("가져오기를 저장하지 못했습니다. 저장 상태를 확인한 뒤 다시 시도하세요.");
    } finally {
      busyRef.current = false;
      if (current === generation.current) setBusy(false);
    }
  };
  return (
    <section
      ref={panel}
      className="import-panel"
      aria-label="가져오기"
      onKeyDown={(event) => {
        if (event.key === "Escape" && !event.nativeEvent.isComposing && !busyRef.current) {
          event.preventDefault();
          onClose();
        }
      }}
    >
      <h2>가져오기</h2>
      <button type="button" disabled={busy} onClick={onClose}>
        닫기
      </button>
      <div>
        <button
          type="button"
          disabled={busy}
          aria-pressed={mode === "curl"}
          onClick={() => {
            reset();
            setMode("curl");
          }}
        >
          curl 붙여넣기
        </button>
        <button
          type="button"
          disabled={busy}
          aria-pressed={mode === "file"}
          onClick={() => {
            reset();
            setMode("file");
          }}
        >
          파일에서
        </button>
      </div>
      {mode === "curl" ? (
        <>
          <textarea
            aria-label="curl 명령"
            value={curl}
            disabled={busy}
            maxLength={16 * 1024 * 1024}
            onChange={(event) => {
              reset();
              setCurl(event.currentTarget.value);
            }}
          />
          <button type="button" disabled={busy || !curl.trim()} onClick={() => void read()}>
            미리 보기
          </button>
        </>
      ) : (
        <>
          <select
            aria-label="가져오기 형식"
            value={format}
            disabled={busy}
            onChange={(event) => {
              reset();
              setFormat(event.currentTarget.value as NativeImportFileFormat);
            }}
          >
            {(
              [
                ["auto", "자동"],
                ["postman", "Postman"],
                ["insomnia", "Insomnia"],
                ["har", "HAR"],
                ["bruno", "Bruno"],
                ["devbox", "Devbox JSON"],
              ] as const
            ).map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
          <button type="button" disabled={busy} onClick={() => void read()}>
            파일 선택
          </button>
        </>
      )}
      {error && <p role="alert">{error}</p>}
      {preview && (
        <>
          {preview.warnings.map((warning, index) => (
            <p key={`${index}:${warning}`}>{warning}</p>
          ))}
          {[...new Set(preview.collections.collections.map((item) => item.folder))].map((folder) => (
            <fieldset key={folder}>
              <legend>{folder || "기본 폴더"}</legend>
              {preview.collections.collections
                .filter((item) => item.folder === folder)
                .map((item) => (
                  <label key={item.id}>
                    <input
                      type="checkbox"
                      disabled={busy}
                      checked={selected.has(item.id)}
                      onChange={() =>
                        setSelected((previous) => {
                          const next = new Set(previous);
                          if (next.has(item.id)) next.delete(item.id);
                          else next.add(item.id);
                          return next;
                        })
                      }
                    />
                    {item.name} · {item.request.url}
                    {item.requiresSecretReview && <span> 비밀 검토 필요</span>}
                  </label>
                ))}
            </fieldset>
          ))}
          <p>환경 {preview.environments.environments.length}개</p>
          {preview.environments.environments.length > 0 && (
            <label>
              <input
                type="checkbox"
                disabled={busy}
                checked={includeEnvironments}
                onChange={(event) => setIncludeEnvironments(event.currentTarget.checked)}
              />
              환경도 가져오기
            </label>
          )}
          <button
            type="button"
            disabled={busy || (!selected.size && !(includeEnvironments && preview.environments.environments.length))}
            onClick={() => void apply()}
          >
            선택한 {selected.size}개 가져오기
          </button>
        </>
      )}
      {toast}
    </section>
  );
}

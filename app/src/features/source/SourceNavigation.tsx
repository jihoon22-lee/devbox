import type { ReactNode } from "react";
import s from "./source.module.css";
export type SourceSection = "files" | "changes" | "branches";
const sections = [
  ["files", "파일"],
  ["changes", "변경"],
  ["branches", "브랜치"],
] as const;
export function SourceNavigation({
  section,
  onChange,
  children,
}: {
  section: SourceSection;
  onChange: (section: SourceSection) => void;
  children: ReactNode;
}) {
  return (
    <aside className={s.sidebar} aria-label="소스 탐색">
      <div role="tablist" aria-label="소스 보기" className={s.toolbar}>
        {sections.map(([key, label], index) => (
          <button
            type="button"
            role="tab"
            id={`source-${key}`}
            key={key}
            aria-selected={section === key}
            aria-controls="source-panel"
            tabIndex={section === key ? 0 : -1}
            onClick={() => onChange(key)}
            onKeyDown={(event) => {
              const next =
                event.key === "Home"
                  ? 0
                  : event.key === "End"
                    ? 2
                    : event.key === "ArrowRight"
                      ? (index + 1) % 3
                      : event.key === "ArrowLeft"
                        ? (index + 2) % 3
                        : -1;
              if (next < 0) return;
              event.preventDefault();
              const nextKey = sections[next]![0];
              onChange(nextKey);
              document.getElementById(`source-${nextKey}`)?.focus();
            }}
          >
            {label}
          </button>
        ))}
      </div>
      <div role="tabpanel" id="source-panel" aria-labelledby={`source-${section}`} tabIndex={0}>
        {children}
      </div>
    </aside>
  );
}

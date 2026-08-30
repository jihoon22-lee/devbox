import { formatPreviewItem } from "../lib/jobEditor";
import type { CronPreviewItem } from "../types";

interface NextRunsPreviewProps {
  items: CronPreviewItem[];
  loading?: boolean;
  error?: string | null;
}

export default function NextRunsPreview({ items, loading = false, error = null }: NextRunsPreviewProps) {
  return (
    <section className="preview-card" aria-labelledby="next-runs-title">
      <div className="section-heading">
        <div>
          <span className="eyebrow">로컬 시간</span>
          <h3 id="next-runs-title">다음 실행</h3>
        </div>
        <span className="preview-count">{items.length}/5</span>
      </div>
      {loading ? <p className="muted">계산 중…</p> : null}
      {error ? (
        <p className="field-error" role="status">
          미리보기를 계산하지 못했습니다: {error}
        </p>
      ) : null}
      {!loading && !error && items.length === 0 ? <p className="muted">표현식을 입력하면 표시됩니다.</p> : null}
      {items.length > 0 ? (
        <ol className="preview-list">
          {items.map((item) => (
            <li key={`${item.wallKey}-${item.timestampMillis}`}>
              <time dateTime={item.datetime}>{formatPreviewItem(item)}</time>
              <span>{item.wallKey}</span>
            </li>
          ))}
        </ol>
      ) : null}
    </section>
  );
}

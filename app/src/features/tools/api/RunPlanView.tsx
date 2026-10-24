/** 실행 전 순서와 생산→소비 관계(어느 요청의 캡처를 어느 요청이 쓰는지). 버튼이나 끌어서 순서를 바꾼다. */
import { useMemo, useState } from "react";
import { Button } from "../../../ui";
import s from "./Api.module.css";
import { toSnapshot } from "./model/draft";
import { latestProducer, planRun } from "./runner/plan";
import type { RunEntry } from "./runner/runner";

export function moveItem<T>(items: T[], from: number, to: number): T[] {
  if (from === to || to < 0 || to >= items.length) return items;
  const next = [...items];
  const [item] = next.splice(from, 1);
  next.splice(to, 0, item!);
  return next;
}

export function RunPlanView({
  entries,
  disabled,
  onReorder,
}: {
  entries: RunEntry[];
  disabled: boolean;
  onReorder: (entries: RunEntry[]) => void;
}) {
  const plan = useMemo(
    () => planRun(entries.map((entry) => ({ produces: entry.produces, snapshot: toSnapshot(entry.draft) }))),
    [entries],
  );
  const [dragging, setDragging] = useState<number>();
  const move = (from: number, to: number) => onReorder(moveItem(entries, from, to));
  return (
    <ol aria-label="실행 순서" className={s.plan}>
      {entries.map((entry, index) => {
        const uses = plan.uses[index] ?? [];
        return (
          <li
            key={entry.id}
            aria-label={`${index + 1}. ${entry.name}`}
            className={s.planItem}
            draggable={!disabled}
            onDragStart={() => setDragging(index)}
            onDragOver={(event) => event.preventDefault()}
            onDrop={() => {
              if (dragging !== undefined) move(dragging, index);
              setDragging(undefined);
            }}
          >
            <strong>{`${index + 1}. ${entry.name}`}</strong>
            {entry.produces.length > 0 && <span className={s.muted}>{`만드는 값: ${entry.produces.join(", ")}`}</span>}
            {uses.map((use) => {
              const producer = latestProducer(plan, use.name, index);
              return (
                <span key={use.name} className={s.muted}>
                  {producer === undefined ? `${use.name} ← 세션·환경` : `${use.name} ← ${entries[producer]!.name}`}
                </span>
              );
            })}
            {entry.reviewNeeded && <span className={s.edited}>비밀 검토 필요</span>}
            <span className={s.grow} />
            <Button size="sm" variant="ghost" disabled={disabled || index === 0} onClick={() => move(index, index - 1)}>
              {`${entry.name} 위로`}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              disabled={disabled || index === entries.length - 1}
              onClick={() => move(index, index + 1)}
            >
              {`${entry.name} 아래로`}
            </Button>
          </li>
        );
      })}
    </ol>
  );
}

import { useState } from "react";
import { Tabs } from "../../ui";
import { ActivityDay } from "./ActivityDay";
import { ActivityWeek } from "./ActivityWeek";
import { ActivityControls } from "./Consent";
import { localDate } from "./dates";
import { useActivityStatus } from "./useActivityStatus";
export function ActivityView() {
  const control = useActivityStatus();
  const [date, setDate] = useState(localDate);
  const [view, setView] = useState("day");
  return (
    <div style={{ padding: "var(--space-3)", overflow: "auto" }}>
      <ActivityControls control={control} />
      {control.storage?.state === "storage_failed" && <p role="alert">저장 중단 · {control.storage.pending}건 대기</p>}
      <label>
        활동 날짜{" "}
        <input
          type="date"
          value={date}
          onChange={(event) => {
            if (event.target.value) setDate(event.target.value);
          }}
        />
      </label>
      <Tabs
        label="활동 기간"
        items={[
          { id: "day", label: "일" },
          { id: "week", label: "주" },
        ]}
        active={view}
        onSelect={setView}
      />
      {view === "day" ? <ActivityDay date={date} /> : <ActivityWeek date={date} />}
    </div>
  );
}

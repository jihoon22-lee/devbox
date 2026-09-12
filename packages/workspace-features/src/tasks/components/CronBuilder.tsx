import { CRON_PRESETS } from "../lib/jobEditor";

interface CronBuilderProps {
  value: string;
  onChange: (value: string) => void;
  error?: string;
}

export default function CronBuilder({ value, onChange, error }: CronBuilderProps) {
  const selectedPreset = CRON_PRESETS.find((preset) => preset.expression === value)?.id ?? "custom";

  return (
    <fieldset className="form-section cron-builder">
      <legend>예약 일정</legend>
      <label className="field">
        <span>프리셋</span>
        <select
          aria-label="cron 프리셋"
          value={selectedPreset}
          onChange={(event) => {
            const preset = CRON_PRESETS.find((candidate) => candidate.id === event.target.value);
            if (preset) onChange(preset.expression);
          }}
        >
          {CRON_PRESETS.map((preset) => (
            <option value={preset.id} key={preset.id}>
              {preset.label}
            </option>
          ))}
          <option value="custom">직접 입력</option>
        </select>
      </label>
      <label className="field">
        <span>cron 표현식</span>
        <input
          aria-label="cron 표현식"
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder="*/5 * * * * 또는 0 */5 * * * *"
          aria-invalid={Boolean(error)}
          aria-describedby={error ? "cron-error" : "cron-help"}
        />
        <small id="cron-help" className="field-help">
          표준 5필드 또는 초를 포함한 6필드를 사용할 수 있습니다. 시간대는 시스템 로컬입니다.
        </small>
        {error ? (
          <small id="cron-error" className="field-error" role="alert">
            {error}
          </small>
        ) : null}
      </label>
    </fieldset>
  );
}

import { useEffect, useMemo, useRef, useState } from "react";
import { TOOLS } from "../tools";
import { ToolOutput, ToolTextArea } from "../tools/common";
import {
  detectSmartInput,
  FIXED_SENSITIVE_REASON,
  SMART_DETECTION_LIMITS,
  type SmartCandidate,
} from "./smartDetection";
import {
  pipelineOutputType,
  pipelineCompatibility,
  pipelineErrorMessage,
  PIPELINE_ERROR_MESSAGES,
  PIPELINE_LIMITS,
  isPipelineValueType,
  runPipeline,
  TRANSFORMERS,
  TRANSFORMER_BY_ID,
  type PipelineError,
  type PipelineStep,
  type PipelineValueType,
} from "./transformPipeline";
import {
  createWorkflowPersistence,
  emptyMetadata,
  nextPipelineId,
  recordRecentTool,
  sanitizeWorkflowMetadata,
  toggleFavoriteTool,
  upsertPipeline,
  WORKFLOW_STORAGE_ERROR,
  type WorkflowMetadata,
  type WorkflowPersistence,
} from "./workflowStore";

const TOOL_BY_ID = new Map(TOOLS.map((tool) => [tool.id, tool]));
const TOOL_IDS = new Set(TOOLS.map((tool) => tool.id));
const TRANSFORMER_IDS = new Set(TRANSFORMERS.map((transformer) => transformer.id));

const INPUT_TYPE_LABELS: Readonly<Record<PipelineValueType, string>> = {
  text: "Text",
  json: "JSON",
  jwt: "JWT",
  url: "HTTP(S) URL",
  base64: "Base64",
  base64url: "Base64URL",
  hex: "Hex",
  "url-component": "URL component",
  yaml: "YAML",
  typescript: "TypeScript",
};

const FIXED_CLIPBOARD_ERROR = "워크플로 입력을 clipboard에서 읽지 못했습니다.";

function firstCompatibleTransformerId(inputType: PipelineValueType): string {
  return TRANSFORMERS.find((transformer) => transformer.inputTypes.includes(inputType))?.id
    ?? TRANSFORMERS[0]?.id
    ?? "";
}

function errorForAddStep(
  code: PipelineError["code"],
  currentType: PipelineValueType,
  expectedTypes: readonly PipelineValueType[] = [],
): PipelineError {
  return { code, stepIndex: null, expectedTypes, actualType: currentType };
}

function useWorkflowPersistence(): WorkflowPersistence {
  const persistence = useRef<WorkflowPersistence | null>(null);
  if (persistence.current === null) {
    persistence.current = createWorkflowPersistence({
      toolIds: TOOL_IDS,
      transformerIds: TRANSFORMER_IDS,
    });
  }
  return persistence.current;
}

export interface SmartWorkflowPanelProps {
  readonly activeToolId: string;
  readonly onOpenTool: (toolId: string) => void;
}

/**
 * One explicit local workflow: inspect a bounded draft, choose typed stages,
 * run them on demand, and persist only IDs/timestamps.  No input/output is
 * passed to the metadata store, clipboard, shell, network, or API handoff.
 */
export function SmartWorkflowPanel({ activeToolId, onOpenTool }: SmartWorkflowPanelProps) {
  const persistence = useWorkflowPersistence();
  const mounted = useRef(true);
  const metadataRef = useRef<WorkflowMetadata>(emptyMetadata());
  const saveRevision = useRef(0);
  const [metadata, setMetadata] = useState<WorkflowMetadata>(emptyMetadata);
  const [loaded, setLoaded] = useState(false);
  const [storageWritable, setStorageWritable] = useState(false);
  const [storageError, setStorageError] = useState<string | null>(null);
  const [input, setInput] = useState("");
  const [inputType, setInputType] = useState<PipelineValueType>("text");
  const [steps, setSteps] = useState<PipelineStep[]>([]);
  const [selectedStepId, setSelectedStepId] = useState(() => firstCompatibleTransformerId("text"));
  const [selectedPipelineId, setSelectedPipelineId] = useState<string | null>(null);
  const [output, setOutput] = useState("");
  const [pipelineError, setPipelineError] = useState<PipelineError | null>(null);

  const detection = useMemo(() => detectSmartInput(input), [input]);
  const currentOutputType = useMemo(
    () => pipelineOutputType(inputType, steps),
    [inputType, steps],
  );
  const selectedStep = TRANSFORMER_BY_ID.get(selectedStepId) ?? null;
  const selectedStepCompatible = selectedStep !== null
    && pipelineCompatibility(currentOutputType, selectedStep.id).compatible;

  useEffect(() => {
    mounted.current = true;
    let active = true;
    void persistence.load()
      .then((loadedMetadata) => {
        if (!active || !mounted.current) return;
        const safe = sanitizeWorkflowMetadata(loadedMetadata, {
          toolIds: TOOL_IDS,
          transformerIds: TRANSFORMER_IDS,
        });
        metadataRef.current = safe;
        setMetadata(safe);
        setLoaded(true);
        setStorageWritable(true);
        setStorageError(null);
      })
      .catch(() => {
        if (!active || !mounted.current) return;
        metadataRef.current = emptyMetadata();
        setMetadata(emptyMetadata());
        setLoaded(true);
        setStorageWritable(false);
        setStorageError(WORKFLOW_STORAGE_ERROR);
      });
    return () => {
      active = false;
      mounted.current = false;
      saveRevision.current += 1;
    };
  }, [persistence]);

  useEffect(() => {
    if (!loaded || !storageWritable || !TOOL_BY_ID.has(activeToolId)) return;
    const next = recordRecentTool(metadataRef.current, activeToolId, Date.now(), TOOL_IDS);
    if (next === metadataRef.current) return;
    metadataRef.current = next;
    setMetadata(next);
    const revision = ++saveRevision.current;
    void persistence.save(next).catch(() => {
      if (mounted.current && saveRevision.current === revision) setStorageError(WORKFLOW_STORAGE_ERROR);
    });
  }, [activeToolId, loaded, persistence, storageWritable]);

  const persist = (next: WorkflowMetadata) => {
    if (!storageWritable) {
      setStorageError(WORKFLOW_STORAGE_ERROR);
      return;
    }
    const safe = sanitizeWorkflowMetadata(next, {
      toolIds: TOOL_IDS,
      transformerIds: TRANSFORMER_IDS,
    });
    metadataRef.current = safe;
    setMetadata(safe);
    setStorageError(null);
    const revision = ++saveRevision.current;
    void persistence.save(safe).catch(() => {
      if (mounted.current && saveRevision.current === revision) setStorageError(WORKFLOW_STORAGE_ERROR);
    });
  };

  const changeInput = (value: string) => {
    if (value.length > SMART_DETECTION_LIMITS.maxInputCodeUnits) {
      setPipelineError(errorForAddStep("invalid_input", inputType));
      return;
    }
    setInput(value);
    setOutput("");
    setPipelineError(null);
  };

  const changeInputType = (value: PipelineValueType) => {
    const nextType = isPipelineValueType(value) ? value : "text";
    setInputType(nextType);
    setSteps([]);
    setSelectedStepId(firstCompatibleTransformerId(nextType));
    setSelectedPipelineId(null);
    setOutput("");
    setPipelineError(null);
  };

  const applyCandidate = (candidate: SmartCandidate) => {
    const transformer = TRANSFORMER_BY_ID.get(candidate.transformerId);
    if (
      transformer === undefined
      || !isPipelineValueType(candidate.inputType)
      || !transformer.inputTypes.includes(candidate.inputType)
    ) return;
    setInputType(candidate.inputType);
    const nextSteps = [{ transformerId: candidate.transformerId }];
    setSteps(nextSteps);
    setSelectedStepId(firstCompatibleTransformerId(transformer.outputType));
    setSelectedPipelineId(null);
    setOutput("");
    setPipelineError(null);
  };

  const addStep = () => {
    if (steps.length >= PIPELINE_LIMITS.maxSteps) {
      setPipelineError(errorForAddStep("too_many_steps", currentOutputType));
      return;
    }
    if (!selectedStep) {
      setPipelineError(errorForAddStep("unknown_transformer", currentOutputType));
      return;
    }
    const compatibility = pipelineCompatibility(currentOutputType, selectedStep.id);
    if (!compatibility.compatible || compatibility.descriptor === null) {
      setPipelineError(errorForAddStep("type_mismatch", currentOutputType, selectedStep.inputTypes));
      return;
    }
    setSteps((current) => [...current, { transformerId: selectedStep.id }]);
    setSelectedStepId(firstCompatibleTransformerId(selectedStep.outputType));
    setSelectedPipelineId(null);
    setOutput("");
    setPipelineError(null);
  };

  const removeStep = (index: number) => {
    const nextSteps = steps.filter((_, stepIndex) => stepIndex !== index);
    setSteps(nextSteps);
    setSelectedStepId(firstCompatibleTransformerId(pipelineOutputType(inputType, nextSteps)));
    setSelectedPipelineId(null);
    setOutput("");
    setPipelineError(null);
  };

  const run = () => {
    if (!input || steps.length === 0) return;
    const result = runPipeline(input, inputType, steps);
    setOutput(result.output);
    setPipelineError(result.error);
  };

  const savePipeline = () => {
    if (steps.length === 0) return;
    const id = selectedPipelineId ?? nextPipelineId(metadataRef.current);
    if (id === null) {
      setPipelineError(errorForAddStep("transform_failed", inputType));
      return;
    }
    const next = upsertPipeline(metadataRef.current, id, inputType, steps, Date.now());
    if (next === metadataRef.current) {
      setPipelineError(errorForAddStep("transform_failed", inputType));
      return;
    }
    setSelectedPipelineId(id);
    persist(next);
  };

  const loadPipeline = (id: string) => {
    const saved = metadataRef.current.pipelines.find((pipeline) => pipeline.id === id);
    if (!saved) return;
    setInputType(saved.inputType);
    const nextSteps = saved.steps.map((step) => ({ transformerId: step.transformerId }));
    setSteps(nextSteps);
    setSelectedStepId(firstCompatibleTransformerId(pipelineOutputType(saved.inputType, nextSteps)));
    setSelectedPipelineId(saved.id);
    setOutput("");
    setPipelineError(null);
  };

  const displayedDetection = detection.status === "too_large"
    ? "입력은 1,000,000바이트 이하만 감지합니다."
    : detection.status === "empty"
      ? "본문을 입력하면 로컬 구조 감지를 시작합니다."
      : detection.status === "ambiguous"
        ? "여러 형식이 가능하므로 추천을 자동 선택하지 않았습니다."
        : detection.status === "unsupported"
          ? "지원되는 안전한 구조를 찾지 못했습니다. 경로와 외부 요청은 실행하지 않습니다."
          : `${detection.inputBytes.toLocaleString("en-US")} bytes · ${detection.candidates.length}개 후보`;

  return (
    <section className="smart-workflow" aria-labelledby="smart-workflow-title">
      <div className="smart-workflow-heading">
        <div>
          <h2 id="smart-workflow-title">Smart Workflows</h2>
          <p id="smart-workflow-help" className="smart-workflow-help">
            bounded 입력을 로컬에서 감지하고, 명시적으로 실행한 typed 변환만 연결합니다. 입력·출력은
            저장하지 않으며 URL을 열거나 shell/API handoff를 실행하지 않습니다.
          </p>
        </div>
        <span className="smart-workflow-mode" role="status">offline</span>
      </div>

      <div className="smart-workflow-input-section">
        <div className="io-label">
          Smart input
          <span className="smart-workflow-byte-limit">최대 {SMART_DETECTION_LIMITS.maxInputBytes.toLocaleString("en-US")} bytes</span>
        </div>
        <ToolTextArea
          aria-label="Smart workflow input"
          aria-describedby="smart-workflow-help"
          className="io-input smart-workflow-input"
          placeholder="JSON, JWT, URL, Base64 또는 Hex를 입력하세요..."
          rows={5}
          value={input}
          onValueChange={changeInput}
          maxLength={SMART_DETECTION_LIMITS.maxInputCodeUnits}
          maxPasteBytes={SMART_DETECTION_LIMITS.maxInputBytes}
          clipboardErrorMessage={FIXED_CLIPBOARD_ERROR}
          spellCheck={false}
        />
        {detection.sensitive ? <div className="smart-workflow-sensitive" role="note">{FIXED_SENSITIVE_REASON}</div> : null}
        {detection.status === "too_large" ? <div className="smart-workflow-error" role="alert">{displayedDetection}</div> : null}
      </div>

      <section className="smart-workflow-detection" aria-labelledby="smart-workflow-detection-title">
        <div id="smart-workflow-detection-title" className="smart-workflow-section-title">Detection</div>
        <div className="smart-workflow-detection-status" role="status" aria-live="polite">{displayedDetection}</div>
        {detection.candidates.length > 0 ? (
          <div className="smart-workflow-candidates" role="list">
            {detection.candidates.map((candidate) => (
              <article className="smart-workflow-candidate" key={candidate.kind} role="listitem">
                <div className="smart-workflow-candidate-copy">
                  <strong id={`smart-workflow-candidate-${candidate.kind}`}>{candidate.label}</strong>
                  <span id={`smart-workflow-candidate-${candidate.kind}-description`}>{candidate.reason}</span>
                  <small>{Math.round(candidate.confidence * 100)}% confidence</small>
                </div>
                <div className="smart-workflow-candidate-actions">
                  <button
                    type="button"
                    className="btn"
                    aria-describedby={`smart-workflow-candidate-${candidate.kind}-description`}
                    onClick={() => applyCandidate(candidate)}
                  >
                    추천 단계로 사용
                  </button>
                  <button
                    type="button"
                    className="btn"
                    aria-describedby={`smart-workflow-candidate-${candidate.kind}-description`}
                    onClick={() => onOpenTool(candidate.toolId)}
                  >
                    도구 열기
                  </button>
                </div>
              </article>
            ))}
          </div>
        ) : null}
      </section>

      <section className="smart-workflow-pipeline" aria-labelledby="smart-workflow-pipeline-title">
        <div id="smart-workflow-pipeline-title" className="smart-workflow-section-title">Typed pipeline</div>
        <div className="smart-workflow-pipeline-toolbar">
          <label>
            입력 형식
            <select aria-label="파이프라인 입력 형식" value={inputType} onChange={(event) => changeInputType(event.currentTarget.value as PipelineValueType)}>
              {(Object.keys(INPUT_TYPE_LABELS) as PipelineValueType[]).map((type) => (
                <option key={type} value={type}>{INPUT_TYPE_LABELS[type]}</option>
              ))}
            </select>
          </label>
          <span className="smart-workflow-arrow" aria-hidden="true">→</span>
          <label className="smart-workflow-add-step">
            다음 단계
            <select aria-label="변환 단계 추가" value={selectedStepId} onChange={(event) => setSelectedStepId(event.currentTarget.value)}>
              {TRANSFORMERS.map((transformer) => {
                const compatibility = pipelineCompatibility(currentOutputType, transformer.id);
                return (
                  <option key={transformer.id} value={transformer.id} disabled={!compatibility.compatible}>
                    {transformer.label}{compatibility.compatible ? "" : " · 형식 불일치"}
                  </option>
                );
              })}
            </select>
          </label>
          <button
            type="button"
            className="btn"
            onClick={addStep}
            disabled={!selectedStepCompatible || steps.length >= PIPELINE_LIMITS.maxSteps}
          >
            단계 추가
          </button>
        </div>

        <ol className="smart-workflow-step-list" aria-label="현재 파이프라인 단계">
          {steps.length === 0 ? <li className="smart-workflow-empty-step">감지 후보를 선택하거나 단계를 추가하세요.</li> : null}
          {steps.map((step, index) => {
            const transformer = TRANSFORMER_BY_ID.get(step.transformerId);
            return (
              <li key={`${step.transformerId}-${index}`} className="smart-workflow-step">
                <span className="smart-workflow-step-number">{index + 1}</span>
                <span id={`smart-workflow-step-${index}`} className="smart-workflow-step-name">{transformer?.label ?? "지원하지 않는 단계"}</span>
                <span className="smart-workflow-step-type">{transformer ? `${transformer.inputTypes.map((type) => INPUT_TYPE_LABELS[type]).join("/")} → ${INPUT_TYPE_LABELS[transformer.outputType]}` : ""}</span>
                <button
                  type="button"
                  className="btn"
                  aria-describedby={`smart-workflow-step-${index}`}
                  onClick={() => removeStep(index)}
                >
                  제거
                </button>
              </li>
            );
          })}
        </ol>

        <div className="smart-workflow-run-toolbar">
          <button type="button" className="btn active" onClick={run} disabled={!input || steps.length === 0}>파이프라인 실행</button>
          <button type="button" className="btn" onClick={savePipeline} disabled={steps.length === 0 || !loaded || !storageWritable}>파이프라인 저장</button>
          <span className="smart-workflow-current-type">현재 출력 형식: {INPUT_TYPE_LABELS[currentOutputType]}</span>
        </div>
        {pipelineError ? (
          <div className="smart-workflow-error" role="alert">
            {pipelineErrorMessage(pipelineError) ?? PIPELINE_ERROR_MESSAGES.transform_failed}
          </div>
        ) : null}
        <div className="smart-workflow-output-label io-label">Pipeline output</div>
        <div aria-live="polite">
          <ToolOutput
            ariaLabel="Pipeline output"
            className="io-output smart-workflow-output"
            value={output}
            actionErrorMessage="파이프라인 결과 작업을 완료하지 못했습니다."
            downloadName="dev-toolbox-pipeline-result.txt"
          />
        </div>
      </section>

      <div className="smart-workflow-library">
        <div className="smart-workflow-library-column">
          <div className="smart-workflow-section-title">Recent tools</div>
          <div className="smart-workflow-chip-list">
            {metadata.recentTools.length === 0 ? <span className="smart-workflow-empty-library">아직 사용한 도구가 없습니다.</span> : null}
            {metadata.recentTools.map((recent) => {
              const tool = TOOL_BY_ID.get(recent.toolId);
              if (!tool) return null;
              return <button type="button" className="smart-workflow-chip" key={recent.toolId} onClick={() => onOpenTool(tool.id)}>{tool.name}</button>;
            })}
          </div>
        </div>
        <div className="smart-workflow-library-column">
          <div className="smart-workflow-section-title">Favorites</div>
          <div className="smart-workflow-chip-list">
            {metadata.favoriteTools.length === 0 ? <span className="smart-workflow-empty-library">즐겨찾기가 없습니다.</span> : null}
            {metadata.favoriteTools.map((toolId) => {
              const tool = TOOL_BY_ID.get(toolId);
              if (!tool) return null;
              return (
                <button type="button" className="smart-workflow-chip favorite" key={tool.id} onClick={() => onOpenTool(tool.id)}>
                  ★ {tool.name}
                </button>
              );
            })}
          </div>
        </div>
        <div className="smart-workflow-library-column smart-workflow-library-pipelines">
          <div className="smart-workflow-section-title">Saved pipelines</div>
          <div className="smart-workflow-chip-list">
            {metadata.pipelines.length === 0 ? <span className="smart-workflow-empty-library">저장된 파이프라인이 없습니다.</span> : null}
            {metadata.pipelines.map((pipeline) => (
              <button type="button" className={`smart-workflow-chip ${pipeline.id === selectedPipelineId ? "selected" : ""}`} key={pipeline.id} onClick={() => loadPipeline(pipeline.id)}>
                {pipeline.id}: {pipeline.steps.map((step) => TRANSFORMER_BY_ID.get(step.transformerId)?.label ?? "지원하지 않는 단계").join(" → ")}
              </button>
            ))}
          </div>
        </div>
      </div>

      <div className="smart-workflow-favorite-action">
        <button
          type="button"
          className="btn"
          aria-pressed={metadata.favoriteTools.includes(activeToolId)}
          onClick={() => persist(toggleFavoriteTool(metadataRef.current, activeToolId, TOOL_IDS))}
          disabled={!TOOL_BY_ID.has(activeToolId) || !loaded || !storageWritable}
        >
          {metadata.favoriteTools.includes(activeToolId) ? "현재 도구 즐겨찾기 해제" : "현재 도구 즐겨찾기"}
        </button>
        {storageError ? <span className="smart-workflow-storage-error" role="alert">{storageError}</span> : null}
      </div>
    </section>
  );
}

export { INPUT_TYPE_LABELS };

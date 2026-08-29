import { useEffect, useState } from "react";
import {
  getMcpValueAtPath,
  initialMcpFieldValue,
  removeMcpValueAtPath,
  setMcpValueAtPath,
} from "./lib/mcp";

interface McpSchemaEditorProps {
  schema: Record<string, unknown>;
  value: Record<string, unknown>;
  disabled: boolean;
  onChange: (value: Record<string, unknown>) => void;
}

export function McpSchemaEditor({ schema, value, disabled, onChange }: McpSchemaEditorProps) {
  return (
    <div className="mcp-schema-editor" aria-label="MCP tool arguments">
      <ObjectFields
        schema={schema}
        root={value}
        path={[]}
        disabled={disabled}
        onChange={onChange}
      />
    </div>
  );
}

function ObjectFields({
  schema,
  root,
  path,
  disabled,
  onChange,
}: {
  schema: Record<string, unknown>;
  root: Record<string, unknown>;
  path: string[];
  disabled: boolean;
  onChange: (value: Record<string, unknown>) => void;
}) {
  const properties = (schema.properties ?? {}) as Record<string, Record<string, unknown>>;
  const required = new Set(schema.required as string[] | undefined);
  if (Object.keys(properties).length === 0) {
    return <div className="dim">이 tool은 arguments가 없습니다.</div>;
  }
  return (
    <div className="mcp-schema-fields">
      {Object.entries(properties).map(([name, child]) => {
        const fieldPath = [...path, name];
        const current = getMcpValueAtPath(root, fieldPath);
        const isRequired = required.has(name);
        const enabled = current !== undefined;
        return (
          <fieldset className="mcp-schema-field" key={name} disabled={disabled}>
            <legend>
              <code>{name}</code>
              {isRequired ? <span className="mcp-required">필수</span> : (
                <label className="mcp-optional-toggle">
                  <input
                    type="checkbox"
                    checked={enabled}
                    onChange={(event) => onChange(event.currentTarget.checked
                      ? setMcpValueAtPath(root, fieldPath, initialMcpFieldValue(child))
                      : removeMcpValueAtPath(root, fieldPath))}
                  />
                  사용
                </label>
              )}
            </legend>
            {typeof child.description === "string" && (
              <div className="dim mcp-schema-description">{child.description}</div>
            )}
            {(isRequired || enabled) && (
              <SchemaValue
                schema={child}
                value={current ?? initialMcpFieldValue(child)}
                root={root}
                path={fieldPath}
                disabled={disabled}
                onChange={onChange}
              />
            )}
          </fieldset>
        );
      })}
    </div>
  );
}

function SchemaValue({
  schema,
  value,
  root,
  path,
  disabled,
  onChange,
}: {
  schema: Record<string, unknown>;
  value: unknown;
  root: Record<string, unknown>;
  path: string[];
  disabled: boolean;
  onChange: (value: Record<string, unknown>) => void;
}) {
  const setValue = (next: unknown) => onChange(setMcpValueAtPath(root, path, next));
  const choices = Array.isArray(schema.enum) ? schema.enum : null;
  if (choices) {
    return (
      <select
        aria-label={`${path.join(".")} enum`}
        value={JSON.stringify(value)}
        disabled={disabled}
        onChange={(event) => setValue(JSON.parse(event.currentTarget.value))}
      >
        {choices.map((choice) => (
          <option key={JSON.stringify(choice)} value={JSON.stringify(choice)}>
            {String(choice)}
          </option>
        ))}
      </select>
    );
  }
  switch (schema.type) {
    case "object":
      return (
        <ObjectFields
          schema={schema}
          root={root}
          path={path}
          disabled={disabled}
          onChange={onChange}
        />
      );
    case "string":
      return (
        <input
          aria-label={`${path.join(".")} string`}
          type="text"
          value={typeof value === "string" ? value : ""}
          minLength={typeof schema.minLength === "number" ? schema.minLength : undefined}
          maxLength={typeof schema.maxLength === "number" ? schema.maxLength : undefined}
          disabled={disabled}
          onChange={(event) => setValue(event.currentTarget.value)}
          spellCheck={false}
        />
      );
    case "integer":
    case "number":
      return (
        <input
          aria-label={`${path.join(".")} ${schema.type}`}
          type="number"
          step={schema.type === "integer" ? 1 : "any"}
          value={typeof value === "number" ? value : 0}
          min={typeof schema.minimum === "number" ? schema.minimum : undefined}
          max={typeof schema.maximum === "number" ? schema.maximum : undefined}
          disabled={disabled}
          onChange={(event) => {
            const number = Number(event.currentTarget.value);
            if (Number.isFinite(number)) setValue(number);
          }}
        />
      );
    case "boolean":
      return (
        <label className="mcp-boolean-field">
          <input
            aria-label={`${path.join(".")} boolean`}
            type="checkbox"
            checked={value === true}
            disabled={disabled}
            onChange={(event) => setValue(event.currentTarget.checked)}
          />
          {value === true ? "true" : "false"}
        </label>
      );
    case "array":
      return <ArrayField value={value} disabled={disabled} onChange={setValue} path={path} />;
    default:
      return null;
  }
}

function ArrayField({
  value,
  disabled,
  onChange,
  path,
}: {
  value: unknown;
  disabled: boolean;
  onChange: (value: unknown) => void;
  path: string[];
}) {
  const serialized = JSON.stringify(Array.isArray(value) ? value : [], null, 2);
  const [draft, setDraft] = useState(serialized);
  const [invalid, setInvalid] = useState(false);
  useEffect(() => {
    setDraft(serialized);
    setInvalid(false);
  }, [serialized]);
  return (
    <div className="mcp-array-field">
      <textarea
        aria-label={`${path.join(".")} array JSON`}
        rows={4}
        value={draft}
        disabled={disabled}
        spellCheck={false}
        onChange={(event) => {
          const next = event.currentTarget.value;
          setDraft(next);
          try {
            const parsed: unknown = JSON.parse(next);
            if (!Array.isArray(parsed)) throw new Error("array required");
            setInvalid(false);
            onChange(parsed);
          } catch {
            setInvalid(true);
          }
        }}
      />
      {invalid && <div className="error mcp-inline-error">JSON 배열 형식이 필요합니다.</div>}
    </div>
  );
}

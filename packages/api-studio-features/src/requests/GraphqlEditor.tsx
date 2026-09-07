import type { GraphqlRequest } from "./types";

interface GraphqlEditorProps {
  value: GraphqlRequest;
  onChange: (value: GraphqlRequest) => void;
}

/** GraphQL fields remain separate from the REST body so request history can preserve
 * the query/variables/operationName contract without storing a resolved body. */
export function GraphqlEditor({ value, onChange }: GraphqlEditorProps) {
  return (
    <div className="graphql-editor" aria-label="GraphQL 요청 편집기">
      <label className="graphql-field">
        <span>Query</span>
        <textarea
          className="body-input graphql-query"
          aria-label="GraphQL query"
          rows={10}
          value={value.query}
          onChange={(event) => onChange({ ...value, query: event.currentTarget.value })}
          spellCheck={false}
        />
      </label>
      <label className="graphql-field">
        <span>Variables (JSON 객체, 선택)</span>
        <textarea
          className="body-input graphql-variables"
          aria-label="GraphQL variables"
          rows={6}
          value={value.variables}
          onChange={(event) => onChange({ ...value, variables: event.currentTarget.value })}
          spellCheck={false}
        />
      </label>
      <label className="graphql-field graphql-operation-name">
        <span>Operation name (선택)</span>
        <input
          aria-label="GraphQL operation name"
          value={value.operation_name}
          onChange={(event) => onChange({ ...value, operation_name: event.currentTarget.value })}
          spellCheck={false}
        />
      </label>
      <p className="graphql-hint">
        네이티브 HTTP 전송만 지원합니다. POST는 JSON 본문을 보내고 GET은 인코딩된 쿼리 매개변수를 보냅니다.
        저장된 쿼리, subscription, 스키마/인트로스펙션 탐색기와 코드 생성은 지원하지 않습니다.
      </p>
    </div>
  );
}

import type { GraphqlRequest } from "./types";

interface GraphqlEditorProps {
  value: GraphqlRequest;
  onChange: (value: GraphqlRequest) => void;
}

/** GraphQL fields remain separate from the REST body so request history can preserve
 * the query/variables/operationName contract without storing a resolved body. */
export function GraphqlEditor({ value, onChange }: GraphqlEditorProps) {
  return (
    <div className="graphql-editor" aria-label="GraphQL request editor">
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
        <span>Variables (JSON object, optional)</span>
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
        <span>Operation name (optional)</span>
        <input
          aria-label="GraphQL operation name"
          value={value.operation_name}
          onChange={(event) => onChange({ ...value, operation_name: event.currentTarget.value })}
          spellCheck={false}
        />
      </label>
      <p className="graphql-hint">
        Native HTTP transport only. POST sends a JSON body; GET sends encoded query parameters.
        Persisted queries, subscriptions, schema/introspection explorer and code generation are not included.
      </p>
    </div>
  );
}

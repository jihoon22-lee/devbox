import { describe, expect, it } from "vitest";
import {
  buildGraphqlBody,
  buildGraphqlGetUrl,
  extractGraphqlCredentialLiterals,
  GRAPHQL_CREDENTIAL_QUERY_ERROR,
  GRAPHQL_ENDPOINT_ERROR,
  GRAPHQL_HEADER_ROWS_ERROR,
  GRAPHQL_URL_TOO_LARGE,
  GRAPHQL_OPERATION_INVALID,
  GRAPHQL_UNSUPPORTED_INTROSPECTION,
  GRAPHQL_UNSUPPORTED_SUBSCRIPTION,
  GRAPHQL_VARIABLES_INVALID,
  parseGraphqlVariables,
  projectGraphqlResponse,
  validateGraphqlHeaders,
  validateGraphqlEndpoint,
  type GraphqlRequest,
  maskGraphqlQueryLiterals,
} from "./graphql";

function request(overrides: Partial<GraphqlRequest> = {}): GraphqlRequest {
  return { query: "{ viewer { id } }", variables: "", operation_name: "", ...overrides };
}

describe("GraphQL wire contract", () => {
  it("builds deterministic JSON body and encoded GET parameters", () => {
    const value = request({
      query: "query Viewer($id: ID!) { viewer(id: $id) { id } }",
      variables: '{"id":"42"}',
      operation_name: "Viewer",
    });
    expect(buildGraphqlBody(value)).toBe(
      '{"operationName":"Viewer","query":"query Viewer($id: ID!) { viewer(id: $id) { id } }","variables":{"id":"42"}}',
    );
    const url = new URL(buildGraphqlGetUrl("https://api.example.test/graphql", [], value));
    expect(url.searchParams.get("query")).toContain("query Viewer");
    expect(url.searchParams.get("variables")).toBe('{"id":"42"}');
    expect(url.searchParams.get("operationName")).toBe("Viewer");
    expect(buildGraphqlBody(request({ variables: '{"z":1,"a":{"d":2,"c":3}}' }))).toBe(
      '{"query":"{ viewer { id } }","variables":{"a":{"c":3,"d":2},"z":1}}',
    );
  });

  it("requires an operation selection and rejects unsupported subscription/introspection", () => {
    expect(() => buildGraphqlBody(request({ query: "query A { a } query B { b }" }))).toThrow(GRAPHQL_OPERATION_INVALID);
    expect(buildGraphqlBody(request({
      query: "query A { a } query B { b }",
      operation_name: "B",
    }))).toContain('"operationName":"B"');
    expect(() => buildGraphqlBody(request({
      query: "query A { a } { b }",
      operation_name: "A",
    }))).toThrow(GRAPHQL_OPERATION_INVALID);
    expect(() => buildGraphqlBody(request({ query: "subscription Events { events }", operation_name: "Events" }))).toThrow(GRAPHQL_UNSUPPORTED_SUBSCRIPTION);
    expect(() => buildGraphqlBody(request({ query: "{ __schema { queryType { name } } }" }))).toThrow(GRAPHQL_UNSUPPORTED_INTROSPECTION);
  });

  it("accepts only bounded JSON objects for variables and rejects credential query params", () => {
    expect(() => parseGraphqlVariables("[]")).toThrow(GRAPHQL_VARIABLES_INVALID);
    expect(() => buildGraphqlGetUrl("https://api.example.test/graphql", [{ key: "access_token", value: "secret" }], request())).toThrow(GRAPHQL_CREDENTIAL_QUERY_ERROR);
    expect(parseGraphqlVariables(" ")).toEqual({});
    expect(() => validateGraphqlHeaders(
      Array.from({ length: 101 }, (_, index) => ({ key: `x-${index}`, value: "ok" })),
    )).toThrow(GRAPHQL_HEADER_ROWS_ERROR);
    expect(() => validateGraphqlEndpoint("https://api.example.test/graphql#fragment"))
      .toThrow(GRAPHQL_ENDPOINT_ERROR);
  });

  it("masks query literals without changing exact environment references", () => {
    expect(maskGraphqlQueryLiterals(
      'query Viewer { viewer(token: "raw-secret", id: "{{ID}}") { id } }',
    )).toBe('query Viewer { viewer(token: "[REDACTED]", id: "{{ID}}") { id } }');
    expect(maskGraphqlQueryLiterals('query Viewer { viewer(token: "unterminated')).toBe(
      'query Viewer { viewer(token: "[REDACTED]"',
    );
    expect(extractGraphqlCredentialLiterals(
      'query Viewer { viewer(token: "escaped\\\"secret", id: "42") { id } }',
    )).toEqual(['escaped"secret']);
  });

  it("fails closed for oversized encoded GET URLs and projects oversized errors", () => {
    expect(() => buildGraphqlGetUrl(
      "https://api.example.test/graphql",
      [],
      request({ query: `{ viewer(filter: "${"x".repeat(9000)}") { id } }` }),
    )).toThrow(GRAPHQL_URL_TOO_LARGE);
    expect(projectGraphqlResponse(
      JSON.stringify({ errors: [{ message: "x".repeat(4097) }] }),
    ).envelope).toBe("oversized");
  });

  it("rejects pathological JSON nesting before recursive parsing", () => {
    let variables = "{";
    for (let index = 0; index < 35; index += 1) variables += '{"next":';
    variables += "0" + "}".repeat(35);
    expect(() => parseGraphqlVariables(variables)).toThrow("GraphQL variables 구조가 허용된 한계를 초과했습니다");

    let response = '{"data":';
    response += "{".repeat(68) + "0" + "}".repeat(68);
    expect(projectGraphqlResponse(response).envelope).toBe("oversized");
  });

  it("projects data and errors without extensions", () => {
    const projected = projectGraphqlResponse(
      '{"data":{"viewer":{"id":"42"}},"errors":[{"message":"bad","path":["viewer",0],"extensions":{"token":"secret"}}]}',
    );
    expect(projected.envelope).toBe("valid");
    expect(projected.data).toEqual({ viewer: { id: "42" } });
    expect(projected.errors[0]).toEqual({ message: "bad", locations: [], path: ["viewer", "0"] });
    expect(JSON.stringify(projected)).not.toContain("extensions");
    expect(projectGraphqlResponse("not-json").envelope).toBe("not_json");
  });
});

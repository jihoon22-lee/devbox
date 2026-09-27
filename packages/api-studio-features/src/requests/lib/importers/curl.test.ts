import { describe, expect, it } from "vitest";
import { parseCurl, tokenizeShell } from "./curl";

describe("curl import", () => {
  it("tokenizes shell quotes, escapes and continuations without executing anything", () => {
    expect(tokenizeShell("curl 'a b' \"c \\\"d\\\"\" $'e\\nf' \\\n -H x")).toEqual([
      "curl",
      "a b",
      'c "d"',
      "e\nf",
      "-H",
      "x",
    ]);
    expect(tokenizeShell("curl '$(id)' '${HOME}'")).toEqual(["curl", "$(id)", "${HOME}"]);
    expect(() => tokenizeShell("curl 'unclosed")).toThrow();
  });
  it("maps JSON and removes redundant content type", () => {
    const { requests, warnings } = parseCurl(
      `curl -X POST 'https://api.example.com/users?x=1' -H 'Content-Type: application/json' -H 'Accept: application/json' --data-raw '{"name":"kim"}' --compressed`,
    );
    expect(requests[0].request).toMatchObject({
      method: "POST",
      url: "https://api.example.com/users?x=1",
      body_kind: "json",
      body: '{"name":"kim"}',
    });
    expect(requests[0].request.headers.map((h) => h.key)).toEqual(["Accept"]);
    expect(warnings).toEqual([]);
  });
  it("infers POST, maps form rows and puts -G data in the query", () => {
    expect(parseCurl("curl https://x.test -d a=1 -d b=2").requests[0].request).toMatchObject({
      method: "POST",
      body_kind: "form",
      body: "a=1\nb=2",
    });
    expect(parseCurl("curl -G https://x.test/s --data-urlencode 'q=a b'").requests[0].request).toMatchObject({
      method: "GET",
      url: "https://x.test/s?q=a%20b",
      body_kind: "none",
    });
  });
  it("maps auth, cookies and user agent", () => {
    const bearer = parseCurl("curl https://x.test -H 'Authorization: Bearer {{token}}'").requests[0].request;
    expect(bearer.auth).toMatchObject({ kind: "bearer", token: "{{token}}" });
    expect(bearer.headers).toEqual([]);
    const basic = parseCurl("curl -u kim:secret https://x.test -b 'a=1; b=2' -A devbox").requests[0].request;
    expect(basic.auth).toMatchObject({ kind: "basic", username: "kim", password: "secret" });
    expect(basic.cookies).toEqual([
      { name: "a", value: "1", enabled: true },
      { name: "b", value: "2", enabled: true },
    ]);
    expect(basic.headers).toEqual([{ key: "User-Agent", value: "devbox", enabled: true }]);
  });
  it("reads cmd copies and bash copies identically", () => {
    const cmd =
      'curl ^"https://api.example.com/users^" ^\n -H ^"Accept: application/json^" ^\n --data-raw ^"^{^\\^"a^\\^":1^}^"';
    expect(parseCurl(cmd).requests[0].request).toEqual(
      parseCurl(`curl 'https://api.example.com/users' -H 'Accept: application/json' --data-raw '{"a":1}'`).requests[0]
        .request,
    );
  });
  it("never reads file bodies or retains local file paths", () => {
    const { requests, warnings } = parseCurl(
      "curl -k --data-binary @payload.json -F 'doc=@/home/me/a.pdf' --retry 3 https://x.test",
    );
    expect(requests[0].request.body).toBe("");
    expect(requests[0].request.multipart[0]).toMatchObject({
      kind: "file",
      name: "doc",
      file_name: "a.pdf",
      file_path: "",
    });
    expect(warnings).toEqual(
      expect.arrayContaining([
        "파일 본문(@payload.json)은 가져오지 않았습니다.",
        "파일 파트 doc는 파일을 다시 선택해야 합니다.",
        "인증서 검증 끄기(-k)는 가져오지 않았습니다.",
        "지원하지 않는 옵션 --retry를 건너뛰었습니다.",
      ]),
    );
  });
  it("handles attached options, flag groups, HEAD and timeout", () => {
    expect(parseCurl("curl.exe -sSL -XOPTIONS --url x.test --max-time 2").requests[0].request).toMatchObject({
      method: "OPTIONS",
      url: "http://x.test",
      timeout_ms: 2000,
    });
    expect(parseCurl("curl -I x.test").requests[0].request.method).toBe("HEAD");
    expect(() => parseCurl("wget https://x.test")).toThrow("curl 명령이 아닙니다");
    expect(() => parseCurl("curl -H")).toThrow();
    expect(() => parseCurl("curl -X TRACE x.test")).toThrow();
  });
});

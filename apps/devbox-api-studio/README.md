# Devbox API Studio

v0.8의 네 사용자 제품 중 하나다. Requests·Protocols·Webhooks·Transforms를 통합한다.
B01~B08의 owner 수용은 완료됐고, 최종 B09 후보·공개 결과는 [#541](https://github.com/jihoon22-lee/devbox/issues/541)에 기록한다.

## 실행과 개발

- `pnpm --filter devbox-api-studio dev`: 명시적으로 표시된 browser fixture.
- Windows의 `pnpm --filter devbox-api-studio tauri dev`: 실제 native 제품.
- Browser `?route=requests`와 debug `--route=requests`는 개발용 route 선택이다.
  release 권한을 부여하는 옵션이 아니며 native host가 caller·session·route를 다시 검사한다.
- 제품 identity는 `com.devbox.v08.apistudio`, 데이터는 installation별 namespace다.
  시작만으로 legacy source를 초기화하거나 원본 경로에 새 데이터를 쓰지 않는다.
- 배포는 Suite installer 또는 제품 ZIP 전체를 사용한다. [설치·이전·복구 안내](../../docs/windows-guide.md).

## 유지하는 계약

- Request의 header 순서/중복, editor draft, environment와 protocol별 transient 상태를 보존한다.
- Webhook temporary listener와 명시적으로 실행한 service-profile worker의 수명을 분리한다.
- Transform·response diff·mock·Knowledge note 이동은 owner가 검증한 artifact review를 따른다.
  route 전환만으로 자동 send/replay/network side effect를 실행하지 않는다.
- API/Webhook/Toolbox 원본을 WAL-consistent snapshot과 닫힌 WebView profile에서 읽는다.
  원본 DB·설정을 보존하며 미지원 schema·source 변경·destination 충돌을 숨기지 않는다.
- Secret은 raw handoff/argv/log에 넣지 않으며 재연결 상태를 유지한다. 비영속 도구는 사용자
  설정처럼 저장하거나 공유 payload에 포함하지 않는다.

## 구현과 근거

기능 코드는 이 제품 host와 `packages/` feature UI, 이름을 가진 `crates/` engine에 있다.
기존 15개 앱의 standalone shell·Tauri bootstrap은 제거했다. 현재 소유권은
[projects](../../docs/projects.md), 기능/데이터 및 R01–R26/S01–S08 대응은
[acceptance trace](../../docs/v0.8-acceptance.md)를 참조한다.

[이전 개발 단계의 상세 README](../../docs/history/v0.8-development/api-studio.md)는 당시 구현
순서·결정의 역사적 기록이다. 그 문서의 hidden/pending 상태를 현재 배포 상태로 해석하지 않는다.
제품 실행/installer 근거와 deterministic fixture·browser·physical device 검사는 구분한다.

### Review corrections (2026-09-21)

GraphQL redirects permanently stop forwarding the original payload after leaving its origin,
including POST-to-GET URL reconstruction. Secret checks run against the final destination URL.
Webhook history and saved fixtures can send a bounded, redacted projection to Workspace Logs.
The approved Suite installation and peer are required; Workspace asks the user to review the
incoming item before claiming it. Projection lifetime is two minutes, with at most 32 pending
items. No raw capture, filesystem path, clipboard or legacy executable is used as a fallback.
An unavailable/offline receiver leaves the original history/fixture intact. Source process
restart or expiry requires sending a new projection. Windows end-to-end execution evidence is
tracked in `workthrough/2026-09-21-v08-review-corrections.md`; historical producer acceptance
does not establish receiver acceptance.

### HTTP request lifetime

HTTP execution timeout covers multipart body construction, the complete redirect chain and
the final response body. Redirects do not reset this monotonic budget. Template/environment/secret
resolution, request validation, and multipart path/metadata preflight occur before execution
starts; this is not a deadline for all work since the user clicked Send. Protocol connection/idle/
RPC budgets remain separate.
New sends supersede older sends; cancellation routing and retained response headers are
registered in the same order.

### Cancellation and shutdown admission

The native registry explicitly classifies HTTP cancellation, MCP HTTP/stdio cancellation and
disconnection, OAuth cancellation, gRPC cancellation/disconnection, SSE stop, and WebSocket
close/disconnection as controls. Webhook stop/product quit and migration cancellation also use
this class. Controls have eight bounded slots independent of 64 normal component operations.
Normal saturation cannot deny their admission. Component/route/session validation, argument
limits and shared replay protection still run; names or prefixes supplied by a renderer do not
grant control priority. Saturating all eight control slots still returns overload.

MCP stdio stderr has no diagnostic consumer. It is drained concurrently through a 4 KiB buffer,
zeroized after each read and on drop, without retention, logging or IPC publication. This avoids
reassembling secrets split across chunks in an unused diagnostic ring. Stdout protocol handling
and bounded process-tree cleanup remain separate.

웹훅은 chunked 전송과 `Expect: 100-continue`를 지원한다. UTF-8이 아닌 본문은
base64로 기록·fixture 저장하고 재전송 시 원래 바이트로 복원한다. 수신 한도는
1,024,000바이트이며 기록에는 바이너리 앞 192,000바이트까지 남는다. 바이너리는
텍스트 비밀 마스킹 대상이 아니며 API 요청 전달은 거부한다. Logs에는 크기 설명만 전달한다.

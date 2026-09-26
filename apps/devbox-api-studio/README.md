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
- 배포는 Suite installer 또는 제품 ZIP 전체를 사용한다. [설치·복구 안내](../../docs/windows-guide.md).

## 유지하는 계약

- Request의 header 순서/중복, editor draft, environment와 protocol별 transient 상태를 보존한다.
- Webhook temporary listener와 명시적으로 실행한 service-profile worker의 수명을 분리한다.
- Transform·response diff·mock·Knowledge note 이동은 owner가 검증한 artifact review를 따른다.
  route 전환만으로 자동 send/replay/network side effect를 실행하지 않는다.
- 처음 실행하면 가져오기 화면 없이 바로 시작한다. 설치본은 Control Center에서 Suite 활성화를 확정한다.
- Secret은 raw handoff/argv/log에 넣지 않으며 재연결 상태를 유지한다. 비영속 도구는 사용자
  설정처럼 저장하거나 공유 payload에 포함하지 않는다.

## 문서 저장소

컬렉션·HTTP 기록·환경·gRPC 요약·변환 워크플로는 제품별
`%LOCALAPPDATA%\com.devbox.v08.apistudio.i*\api-store.db`에 저장한다.
SQLite WAL과 revision 비교로 저장하며 문서 종류마다 16MiB까지 허용한다.
비밀값은 기존처럼 DPAPI로 봉인하고 요청 저장 전 정화 규칙을 적용한다.
WebView2 데이터 폴더를 초기화해도 이 문서 저장소는 남는다.

첫 실행은 기존 WebView 문서를 저장하고 다시 읽어 확인한 뒤 옛 키를 지운다.
이전에 실패한 종류는 원본을 보존하고 다음 실행까지 쓰기를 차단한다.
기존 `transforms/smart-workflows.json`은 새 문서가 없을 때 복사하며 원본 파일은 남긴다.
Control Center 데이터 checkpoint는 제품 폴더 전체를 정지 상태에서 복사하므로
`api-store.db`와 남아 있는 WAL도 함께 포함한다. Browser fixture는 별도 미리보기 저장소를 쓴다.

## 구현과 근거

기능 코드는 이 제품 host와 `packages/` feature UI, 이름을 가진 `crates/` engine에 있다.
기존 15개 앱의 standalone shell·Tauri bootstrap은 제거했다. 현재 소유권은
[projects](../../docs/projects.md), 기능/데이터 및 R01–R26/S01–S08 대응은
[acceptance trace](https://github.com/jihoon22-lee/devbox-archive/blob/main/docs/v0.8-acceptance.md)를 참조한다.

[이전 개발 단계의 상세 README](https://github.com/jihoon22-lee/devbox-archive/blob/main/docs/history/v0.8-development/api-studio.md)는 당시 구현
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
close/disconnection as controls. Webhook stop/product quit also use
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

## 타입 IPC 개발

제품 플러그인의 명령은 `api·webhooks·transforms`다. native enum이 메서드·인자·허용 route를 정하며, 공용 admission이 세션·소유권·동시 실행을 확인한다. TypeScript 계약은 `packages/api-studio-features/src/generated`에 생성한다. 전체 묶음 개발을 마친 뒤 루트 `.github/scripts/check-generated-bindings.sh`를 실행하고 생성 결과를 커밋한다. CI는 Rust exporter와 포맷한 생성물의 차이·새 파일을 검사한다.

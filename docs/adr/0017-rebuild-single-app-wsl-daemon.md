# 0017 재구축: 단일 앱 + WSL 데몬

상태: 채택 · 기록일: 2026-10-12

맥락: 사용 빈도 1위인 에이전트 기능(세션 유지, hook 상태 감지, 알림, 재개)이 v0.9.0 구조에 없었고, 리뷰가 확정한 결함 약 20건 중 에이전트·터미널 결함은 0건이었다. 네 제품·설치 세대·제품 간 연결·Windows 프로세스 처리·WSL helper 중계 및 Windows·WSL 이중 경로 파일은 전체 약 49.7만 줄의 약 25%(파일 단위 하한), `.github`·`scripts`는 5.6만 줄이었다. 사용자는 데이터 이전 없이 WSL 중심 재구축을 결정했다.

결정: `01-design.md`를 원장으로 하는 단일 Windows Tauri 앱 + WSL Linux 데몬 구조로 다시 짓는다. S0a 자동 측정은 위 결과와 같이 합격했다. R1/R2 수동 판정은 처음에 S0b S0 확인에 남겼고, 사용자 결정(2026-10-13)으로 S6 공개 직전 사용자 확인으로 옮겼다. 그 전까지 R1/R2는 NOT_RUN이다. 별도 scope는 PTY 소실의 SIGHUP 생존을 보장하지 않으며 명시적 종료 정책을 유지한다. hook은 Claude 즉시 PermissionRequest와 Codex 주 thread·현재 turn의 검증된 source 계약을 사용한다.

결과: ADR 0001·0002·0003·0004·0006·0007·0008·0009·0010·0011·0012·0013·0014·0015를 대체한다. Windows 설치본 수용, SC3·SC10 전체 경로 및 세션 tracker 회귀는 후속 단계의 gate이며 S0a의 측정 PASS로 대체하지 않는다. 0005와 0016은 유지하며 v1 형태는 다음과 같다.

- 0005 v1 형태: 외부 입력은 프레임 크기(JSON 메시지는 평소 1 MiB 이하·하드 상한 16 MiB, 스트림 조각은 64 KiB)와 스트림 신용 창(256 KiB)으로 제한하고, 상한에 걸려 잘린 결과에는 `truncated` 표시를 붙인다(01-design §5.1·§5.3).
- 0016 v1 형태: 데몬은 unix socket(0600)만 열고(TCP는 사용자가 켠 웹훅 리스너·OAuth 콜백·dev-gateway뿐), 비밀은 DPAPI로 봉인한 키로 암호화한다. 코드 서명·업데이트 서명·네이티브 확인 대화상자·소켓 외 접근 제어는 하지 않는다(01-design §5.1·§12).

근거: [S0a 측정과 제한](https://github.com/jihoon22-lee/devbox/blob/842fa82e403231e67fc4dfb7e0d64b2ecc2af1d2/docs/superpowers/plans/2026-10-08-devbox-v1/09-s0a-results.md), [설계](../superpowers/plans/2026-10-08-devbox-v1/01-design.md).

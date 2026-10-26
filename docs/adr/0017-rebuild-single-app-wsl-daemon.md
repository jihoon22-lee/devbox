# 재구축: 단일 앱 + WSL 데몬

상태: 채택 · 기록일: 2026-10-12

맥락: 사용 빈도 1위인 에이전트 기능(세션 유지, hook 상태 감지, 알림, 재개)이 v0.9.0 구조에 없었고, 리뷰가 확정한 결함 약 20건 중 에이전트·터미널 결함은 0건이었다. 네 제품·설치 세대·제품 간 연결·Windows 프로세스 처리·WSL helper 중계 및 Windows·WSL 이중 경로 파일은 전체 약 49.7만 줄의 약 25%(파일 단위 하한), `.github`·`scripts`는 5.6만 줄이었다. 사용자는 데이터 이전 없이 WSL 중심 재구축을 결정했다.

결정: `01-design.md`를 원장으로 하는 단일 Windows Tauri 앱 + WSL Linux 데몬 구조로 다시 짓는다. S0a 자동 측정은 위 결과와 같이 합격했다. R1/R2 수동 판정은 처음에 S0b S0 확인에 남겼고, 사용자 결정(2026-10-13)으로 S6 공개 직전 사용자 확인으로 옮겼다. 그 전까지 R1/R2는 NOT_RUN이다. 별도 scope는 PTY 소실의 SIGHUP 생존을 보장하지 않으며 명시적 종료 정책을 유지한다. hook은 Claude 즉시 PermissionRequest와 Codex 주 thread·현재 turn의 검증된 source 계약을 사용한다.

결과: ADR 0001·0002·0003·0004·0007·0009·0010·0011·0012·0014·0015를 대체한다. 0005·0006·0008·0013·0016은 01 §13의 형태로 유지한다. Windows 설치본 수용, SC3·SC10 전체 경로 및 세션 tracker 회귀는 후속 단계의 gate이며 이 초안의 측정 PASS로 대체하지 않는다.

근거: [S0a 측정과 제한](../superpowers/plans/2026-10-08-devbox-v1/09-s0a-results.md), [설계](../superpowers/plans/2026-10-08-devbox-v1/01-design.md).

# Devbox Agent

설치된 Suite의 Control Center에 포함되는 창 없는 Tauri 구성요소다. 공개 제품·별도 설치 자산이 아니며 portable 실행은 지원하지 않는다. 검증된 owner의 데이터 namespace와 writer lease를 사용한다.

백그라운드 기능의 구현 계약은 [ADR 0015](../../docs/adr/0015-devbox-agent.md), 진행 원장은 리뷰 후속 ledger #580이다.

Workspace의 작업·서비스·예약·로그와 API Studio의 Webhook 리스너를 소유한다. UI 종료와 명시적인
작업/리스너 중지를 구분한다. 설치 generation과 native peer를 확인한 pipe로만 제품 요청을 받으며,
기존 제품 데이터 namespace를 그대로 사용한다. terminal·LSP·편집기와 제품 간 handoff 발행은 UI host에 남는다.

Knowledge의 동의한 활동 수집·검색 색인도 소유한다. 저장소가 없으면 요청 때 다시 확인하고,
미동의 상태에서는 색인만 시작한다. 트레이 하나에서 제품 열기·수집 일시중지/재개·전체 종료를 제공한다.
명시적 전체 종료 뒤 기존 제품의 조회는 자동 재시작하지 않으며 수동 재연결로 다시 시작할 수 있다.

Control Center 환경과 Knowledge 활동 설정은 같은 기본 꺼짐 로그인 설정을 사용한다.
자동 시작·설치/업데이트 commit은 검증된 현재 generation 경로를 등록하며 제품 창은 만들지 않는다.
Run 키 정책과 Windows adapter는 bootstrap도 사용하는 `suite-runtime::agent_autostart`에 있다.

설치 루트의 소유된 `bin/devbox-mcp.exe` 사본은 `--mcp-stdio`로만 MCP stdio 모드를 시작하며
Tauri 창·트레이·두 번째 agent 서버를 만들지 않는다. 매 호출의 native MCP 역할·현재 generation을
확인하고, 읽기 도구 및 별도로 허용한 노트 캡처/신뢰한 작업 실행만 `agent.mcp`로 전달한다.
MCP 설정은 Control Center 환경에서 변경하며 기본은 모두 꺼짐이다.

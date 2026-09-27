# Devbox Agent

설치된 Suite의 Control Center에 포함되는 창 없는 Tauri 구성요소다. 공개 제품·별도 설치 자산이 아니며 portable 실행은 지원하지 않는다. 검증된 owner의 데이터 namespace와 writer lease를 사용한다.

백그라운드 기능의 구현 계약은 [ADR 0015](../../docs/adr/0015-devbox-agent.md), 진행 원장은 리뷰 후속 ledger #580이다.

Workspace의 작업·서비스·예약·로그와 API Studio의 Webhook 리스너를 소유한다. UI 종료와 명시적인
작업/리스너 중지를 구분한다. 설치 generation과 native peer를 확인한 pipe로만 제품 요청을 받으며,
기존 제품 데이터 namespace를 그대로 사용한다. terminal·LSP·편집기와 제품 간 handoff 발행은 UI host에 남는다.

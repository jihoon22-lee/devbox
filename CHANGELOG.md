# Changelog

이 프로젝트의 모든 주요 변경사항은 이 파일에 기록한다.
형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/)를 따르며, 버전은 `vX.Y.Z` 태그와 함께 릴리스된다.

## [v0.1.1] - 2026-08-11

Windows에서 발견된 버그 수정 릴리스. port-manager / wsl-dashboard / knowledge-base / life-log의 버전을 0.1.1로 올렸다.

### Fixed

- **port-manager** — 한국어 Windows에서 `netstat` 출력(OEM 코드페이지, 예: CP949)이 UTF-8 디코딩을 실패하던 문제 수정. 자식 프로세스의 콘솔 창 깜빡임도 제거.
- **wsl-dashboard** — 파이프로 실행된 `wsl.exe -l -v`가 UTF-16LE(NUL 포함)로 출력돼 "null byte found in provided data" 오류가 나던 문제 수정 (UTF-16 디코딩 추가). wsl/git 자식 프로세스의 콘솔 창 깜빡임도 제거.
- **knowledge-base** — Windows 절대경로 처리 버그로 파일 작업·데일리 노트에서 "경로가 루트 밖을 벗어납니다"가 나던 문제 수정.
- **life-log** — git 커밋 집계 자식 프로세스의 콘솔 창 깜빡임 제거.

## [v0.1.0] - 2026-08-11

최초 릴리스: 8개 데스크톱 앱 (Tauri v2, Rust + React).

### Added

- **port-manager** — 포트/프로세스 조회·검색·필터, 프로세스 종료, localhost 열기
- **developer-toolbox** — 14종 개발 도구 (JSON/Base64/URL/타임스탬프/Case/Hash/UUID/Regex/Diff/JWT)
- **wsl-dashboard** — WSL 배포판·Docker·git 상태 대시보드, 컨테이너 start/stop/restart
- **api-playground** — REST 요청 빌더, CORS 없는 응답 확인, 요청 history
- **activity-timeline** — 포그라운드 창 기반 사용 기록, 하루 타임라인·앱 통계, 트레이 상시 실행
- **everything-plus** — 파일명 FTS5 인덱스·검색, 루트 관리, 백그라운드 재인덱스
- **knowledge-base** — 마크다운 저장소, 태그, 본문 검색, 데일리 노트
- **life-log** — 활동·git 집계 일일 요약, 데이터 소스 설정

### Changed

- 없음 (최초 릴리스)

### Fixed

- 없음 (최초 릴리스)

### Known issues

- 개인 빌드라 코드 서명이 없어 설치 시 SmartScreen 경고가 표시된다 (`추가 정보 → 실행`).
- activity-timeline 포그라운드 추적, wsl-dashboard 등 **Windows 전용 기능은 Windows에서만 동작**한다.
- everything-plus 내용(body) 검색은 v2 예정. 현재는 파일명 검색만 지원.

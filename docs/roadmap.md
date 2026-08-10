# Roadmap

8개 앱을 순차적으로 완성하고, 공통 코드가 실제로 필요해지면 그때 `crates/`·`packages/`로 추출한다.

## Phase 1 — Tauri 기본기
- [ ] **port-manager** — IPC, Rust 기초, 설정, netstat 파싱
- [ ] **developer-toolbox** — React 사이드바 UI, 소형 도구들

> 이 시점에 `packages/ui`(레이아웃)와 `crates/process`를 추출하면 이후 앱에 재사용.

## Phase 2 — 시스템/네트워크
- [ ] **wsl-dashboard** — 자식 프로세스, async 명령, wsl/docker/git 파싱
- [ ] **api-playground** — HTTP(reqwest), 상태관리, 키-값 편집기

## Phase 3 — 데이터/검색
- [ ] **activity-timeline** — SQLite, 백그라운드(트레이), foreground window
- [ ] **everything-plus** — 파일 watcher, FTS5 검색, 성능 최적화

> `crates/database`, `crates/filesystem`, `crates/search` 추출 시점.

## Phase 4 — 개인 데이터 플랫폼
- [ ] **knowledge-base** — 파일 기반 지식 저장소, 태그/백링크
- [ ] **life-log** — 타 앱 데이터 집계 허브 (activity/git/files/notes)

## 통합 (선택)
- [ ] **workbench** — 8개를 하나의 대시보드로 통합

## 현재 상태
- Phase 1 미착수 (계획 문서만 완비)

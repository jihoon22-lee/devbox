# @devbox/diff-view

§12.5 변경 집합 preview 부품. code-pad(crash recovery)·run-manager(definition import)가 공유한다.

- `ChangeSetPreview` — "path → (before, after)" 목록, 항목 단위·전체 단위 승인/폐기
- 실제 적용은 컴포넌트 밖에서 수행 (preview와 선택까지만 책임)

CSS는 사용 앱이 제공한다 (`.changeset-*` 클래스).

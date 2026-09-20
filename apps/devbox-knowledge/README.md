# Devbox Knowledge

v0.8의 네 사용자 제품 중 하나다. Notes·Daily·Activity·Search를 통합한다.
B01~B08의 owner 수용은 완료됐고, 최종 B09 후보·공개 결과는 [#541](https://github.com/jihoon22-lee/devbox/issues/541)에 기록한다.

## 실행과 개발

- `pnpm --filter devbox-knowledge dev`: 명시적으로 표시된 browser fixture.
- Windows의 `pnpm --filter devbox-knowledge tauri dev`: 실제 native 제품.
- Browser `?route=notes`와 debug `--route=notes`는 개발용 route 선택이다.
  release 권한을 부여하는 옵션이 아니며 native host가 caller·session·route를 다시 검사한다.
- 제품 identity는 `com.devbox.v08.knowledge`, 데이터는 installation별 namespace다.
  시작만으로 legacy source를 초기화하거나 원본 경로에 새 데이터를 쓰지 않는다.
- 배포는 Suite installer 또는 제품 ZIP 전체를 사용한다. [설치·이전·복구 안내](../../docs/windows-guide.md).

## 유지하는 계약

- Notes/Daily는 원본 vault·Markdown·assets·templates를 보존한다. 활성 vault 변경은 별도 검토한다.
- Activity의 day boundary/timezone/source provenance를 유지하고 선택한 요약만 preview 후 삽입한다.
- Search는 파일/노트 source별 bounded query·timeout·partial·cancel·freshness와 saved query/root/
  exclusion을 지원한다. 사용자 설정은 파생 index DB와 별도로 이전한다.
- 세 legacy source의 consistent snapshot을 새 generation에 반영한다. source ID와 import receipt는
  재실행에서 destination 편집·삭제를 보존한다. corrupt/future pointer를 자동 초기화하지 않는다.
- 신규 store 시작과 원본 import를 분리한다. 이전의 collection consent·살아 있는 작업은
  자동 재개하지 않으며 API/Activity artifact는 source authority와 명시적 review로 수신한다.

## 구현과 근거

기능 코드는 이 제품 host와 `packages/` feature UI, 이름을 가진 `crates/` engine에 있다.
기존 15개 앱의 standalone shell·Tauri bootstrap은 제거했다. 현재 소유권은
[projects](../../docs/projects.md), 기능/데이터 및 R01–R26/S01–S08 대응은
[acceptance trace](../../docs/v0.8-acceptance.md)를 참조한다.

[이전 개발 단계의 상세 README](../../docs/history/v0.8-development/knowledge.md)는 당시 구현
순서·결정의 역사적 기록이다. 그 문서의 hidden/pending 상태를 현재 배포 상태로 해석하지 않는다.
제품 실행/installer 근거와 deterministic fixture·browser·physical device 검사는 구분한다.

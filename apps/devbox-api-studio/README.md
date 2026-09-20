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

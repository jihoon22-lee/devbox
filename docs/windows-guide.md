# Windows 설치·이전·복구

[GitHub Releases](https://github.com/jihoon22-lee/devbox/releases)에 실제 공개된 버전을 선택한다.
v0.8 소스/후보/공개 상태는 [#541](https://github.com/jihoon22-lee/devbox/issues/541)에 구분해 기록한다.
Windows 11과 WebView2가 필요하다. 일반 사용자는 Rust·Node·MSVC를 설치할 필요가 없다.

## 설치와 portable

Suite 설치 파일 `Devbox_0.8.0_x64-setup.exe`는 Workspace·API Studio·Knowledge·Control Center를
함께 설치한다. 독립 실행이 필요하면 해당 제품 ZIP 전체를 별도 폴더에 푼다. 실행 파일만
옮기면 필수 component와 installation identity가 빠진다. 서로 다른 portable과 설치본은
각자의 namespace/identity로 취급하며 임의 경로를 Suite 구성원으로 자동 등록하지 않는다.

## v0.7에서 이전

기존 앱을 종료하고 Control Center의 이전 검토에서 발견된 원본과 destination을 확인한다.
원본 DB와 WAL을 consistent snapshot으로 읽으며 원본을 덮어쓰거나 이동하지 않는다.
full/partial/mixed 설치 모두 발견된 source별로 적용/명시적 제외를 검토한다. 오래된 검토,
미래 schema·손상·missing source·동시 변경은 진단을 확인하고 새로 검토한다.

Git repository·vault 파일과 assets는 원래 경로에 유지한다. Notes templates와 검색 root·saved query·
exclusion 같은 사용자 설정은 파생 index와 별도로 이전한다. LSP/runtime cache와 살아 있는
process는 복사한 사용자 데이터라고 취급하지 않는다. 보호된 secret은 UI 안내에 따라 재연결한다.

## 업데이트와 복구

Control Center의 review에 표시된 generation과 변경 내용을 확인한 뒤 적용한다. commit 전 undo는
이전 활성 generation으로 돌아가며, data restore는 검토한 경계를 따른다. commit 후 생성한 새
데이터를 과거 backup으로 조용히 덮어쓰지 않는다. 잠긴 파일·권한·공간 부족·중단은 실패 상태와
journal을 남기며 재개/복구 UI에서 처리한다. 복구가 끝나기 전에 설치 폴더를 수동으로 지우지 않는다.

제거는 설치가 소유한 파일·등록만 대상으로 하고 네 제품 사용자 데이터를 보존한다. 기존 앱 정리는
별도 검토한 설치 provenance와 file identity에만 적용한다. 소유권이 바뀐 portable이나 잠긴 원본은
삭제하지 않고 pending/changed 상태를 안내한다. 외부 사용자가 만든 구 exe 절대경로 shortcut이나
script는 자동 리다이렉트되지 않으므로 새 제품 경로로 직접 수정해야 한다.

## 개발·문제 해결

[개발자 가이드](development.md), [v0.8 수용 범위](v0.8-acceptance.md)를 참조한다.
운영 중 Docker·iptables·공유 네트워크를 테스트 준비 목적으로 변경하지 않는다.
과거 설치 방식은 [v0.7 가이드](history/v0.7/windows-guide.md)에 보존한다.

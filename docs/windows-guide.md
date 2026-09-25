# Windows 설치·복구

[GitHub Releases](https://github.com/jihoon22-lee/devbox/releases)에 실제 공개된 버전을 선택한다.
v0.8 소스/후보/공개 상태는 [#541](https://github.com/jihoon22-lee/devbox/issues/541)에 구분해 기록한다.
Windows 11과 WebView2가 필요하다. 일반 사용자는 Rust·Node·MSVC를 설치할 필요가 없다.

## 설치와 portable

Suite 설치 파일 `Devbox_0.8.0_x64-setup.exe`는 Workspace·API Studio·Knowledge·Control Center를
함께 설치한다. 독립 실행이 필요하면 해당 제품 ZIP 전체를 별도 폴더에 푼다. 실행 파일만
옮기면 필수 component와 installation identity가 빠진다. 서로 다른 portable과 설치본은
각자의 namespace/identity로 취급하며 임의 경로를 Suite 구성원으로 자동 등록하지 않는다.

## 새 설치 활성화

새 설치는 설치 후 Control Center의 데이터 및 복구 화면에서 활성화를 확정한다.
네 제품을 열어 저장소를 준비하고 상태 기록을 확인한다.

## 업데이트와 복구

Control Center의 review에 표시된 generation과 변경 내용을 확인한 뒤 적용한다. commit 전 undo는
이전 활성 generation으로 돌아가며, data restore는 검토한 경계를 따른다. commit 후 생성한 새
데이터를 과거 backup으로 조용히 덮어쓰지 않는다. 잠긴 파일·권한·공간 부족·중단은 실패 상태와
journal을 남기며 재개/복구 UI에서 처리한다. 복구가 끝나기 전에 설치 폴더를 수동으로 지우지 않는다.

제거는 설치가 소유한 파일·등록만 대상으로 하고 네 제품 사용자 데이터를 보존한다.
외부에서 만든 실행 파일 절대경로 shortcut이나 script는 새 제품 경로로 직접 수정한다.

## 개발·문제 해결

[개발자 가이드](development.md), [v0.8 수용 범위](https://github.com/jihoon22-lee/devbox-archive/blob/main/docs/v0.8-acceptance.md)를 참조한다.
운영 중 Docker·iptables·공유 네트워크를 테스트 준비 목적으로 변경하지 않는다.
과거 설치 방식은 [v0.7 가이드](https://github.com/jihoon22-lee/devbox-archive/blob/main/docs/history/v0.7/windows-guide.md)에 보존한다.

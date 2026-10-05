# 0006 콘텐츠 파일 identity와 링크

상태: 채택

기록일: 2026-09-25

## 맥락

OneDrive placeholder와 WOF 압축은 이름을 다른 위치로 돌리는 링크가 아니다. ReFS의 파일 구분에는 확장 ID가 필요하다.

## 결정

symlink·junction 등 이름 대리 reparse는 거부하고 storage reparse는 허용한다. 같은 열린 핸들의 128비트 Windows ID를 객체 비교에 사용하고 콘텐츠 proof는 content_components()로 만든다. 설치 기록의 components()는 기존 값을 유지한다.

## 결과

클라우드·압축 파일을 읽으면서 객체 교체를 확인한다. 파일시스템이 확장 API를 지원하지 않으면 기존 ID로 대체하며 실제 OneDrive·Dev Drive 동작은 Windows 실기로 확인한다.

## 근거

- [crates/filesystem/src/lib.rs](../../crates/filesystem/src/lib.rs)
- [crates/filesystem/src/links.rs](../../crates/filesystem/src/links.rs)
- [docs/superpowers/plans/2026-09-23-review-remediation/p0-05-filesystem-links.md](https://github.com/jihoon22-lee/devbox/blob/ab2ee78a54d3aef7a0e2d546349574bc12a4c2ac/docs/superpowers/plans/2026-09-23-review-remediation/p0-05-filesystem-links.md)

현재 구현·검증: [재정비 계획](../superpowers/plans/2026-10-03-product-readiness/00-roadmap.md). 과거 결정: [닫힌 ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).

# 0010 검증한 후보의 그대로 승격

상태: 대체됨(0017)

기록일: 2026-09-25

## 맥락

소스 검사와 실제 배포 bytes 검사는 다른 증거다. 검증한 뒤 다시 빌드하면 같은 산출물이라는 근거가 사라진다.

## 결정

exact-main 후보의 assembly·packaged runtime·installer를 확인하고 같은 commit의 annotated tag로 재빌드 없이 승격한다. 제품·빌드 입력이 같은 fixture-only 보정은 릴리스 정책의 한정된 receipt 절차로 원본 빌드 SHA·bytes와 새 검증 SHA/run을 구분한다. 제품 입력이 바뀌면 새 exact-main 후보가 필요하다. 공개 자산은 Suite setup·네 portable ZIP·manifest·notices의 7개이며 서명 범위는 ADR 0016을 따른다.

## 결과

공개된 bytes를 후보 증거와 연결할 수 있다. 후보가 없거나 만료됐으면 승격하지 않고, 공개 RC는 명시 요청이 있을 때만 만든다.

## 근거

- [docs/release-policy.md](https://github.com/jihoon22-lee/devbox/blob/72ff50c7d992fccb54ca51d796cfa7f5b29c7caa/docs/release-policy.md)
- [.github/workflows/release.yml](https://github.com/jihoon22-lee/devbox/blob/72ff50c7d992fccb54ca51d796cfa7f5b29c7caa/.github/workflows/release.yml)
- [.github/workflows/windows-package-candidate.yml](https://github.com/jihoon22-lee/devbox/blob/72ff50c7d992fccb54ca51d796cfa7f5b29c7caa/.github/workflows/windows-package-candidate.yml)

현재 결정: [ADR 0017](0017-rebuild-single-app-wsl-daemon.md) · [v1 설계](../superpowers/plans/2026-10-08-devbox-v1/01-design.md). 과거 결정: [닫힌 ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).

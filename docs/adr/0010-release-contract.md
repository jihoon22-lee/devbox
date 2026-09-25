# 0010 검증한 후보의 그대로 승격

상태: 채택

기록일: 2026-09-25

## 맥락

소스 검사와 실제 배포 bytes 검사는 다른 증거다. 검증한 뒤 다시 빌드하면 같은 산출물이라는 근거가 사라진다.

## 결정

exact-main 후보의 assembly·packaged runtime·installer를 확인하고 같은 commit의 annotated tag로 재빌드 없이 승격한다. 공개 자산은 Suite setup·네 portable ZIP·manifest·notices의 7개이며 서명 범위는 ADR 0016을 따른다.

## 결과

공개된 bytes를 후보 증거와 연결할 수 있다. 후보가 없거나 만료됐으면 승격하지 않고, 공개 RC는 명시 요청이 있을 때만 만든다.

## 근거

- [docs/release-policy.md](../../docs/release-policy.md)
- [.github/workflows/release.yml](../../.github/workflows/release.yml)
- [.github/workflows/windows-package-candidate.yml](../../.github/workflows/windows-package-candidate.yml)

리뷰 후속 결정·진행: [ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).

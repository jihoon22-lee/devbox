# 아키텍처 결정 기록(ADR)

한 파일에 결정 하나를 남긴다. 번호는 바꾸지 않는다. 결정을 뒤집을 때는 새 ADR을 쓰고 이전 기록의 상태를 `대체됨(NNNN)`으로 바꾼다.

형식은 제목·상태(제안/채택/대체됨)·날짜·맥락·결정·결과·근거다. 이 문서들의 날짜는 기존 결정의 기록일이며, 제안 상태는 구현 완료를 뜻하지 않는다.

| 번호 | 제목 | 상태 |
|---|---|---|
| 0001 | [네 제품 구성](0001-four-products.md) | 채택 |
| 0002 | [세대별 Suite 설치](0002-suite-installation.md) | 채택 |
| 0003 | [native 세션 입장 검사](0003-session-guard.md) | 채택 |
| 0004 | [native 소유 권한](0004-native-owned-authority.md) | 채택 |
| 0005 | [외부 입력과 작업의 상한](0005-bounded-io.md) | 채택 |
| 0006 | [콘텐츠 파일 identity와 링크](0006-filesystem-safety.md) | 채택 |
| 0007 | [설치별 제품 데이터 namespace](0007-data-namespaces.md) | 채택 |
| 0008 | [사용자 범위 비밀 봉인](0008-secrets.md) | 채택 |
| 0009 | [같은 설치 제품의 자동 연결](0009-suite-connection.md) | 채택 |
| 0010 | [검증한 후보의 그대로 승격](0010-release-contract.md) | 채택 |
| 0011 | [과제 회귀와 묶음 최종 검증](0011-verification-policy.md) | 채택 |
| 0012 | [고정 스키마 운영 로그](0012-operation-log.md) | 채택 |
| 0013 | [v0.7 가져오기 종료](0013-drop-v07-migration.md) | 채택 |
| 0014 | [component별 타입 IPC](0014-typed-ipc.md) | 제안 |
| 0015 | [사용자별 백그라운드 agent](0015-devbox-agent.md) | 제안 |
| 0016 | [개인용 보안 범위](0016-personal-security-scope.md) | 채택 |

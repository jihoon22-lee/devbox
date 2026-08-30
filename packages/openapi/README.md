# @devbox/openapi

앱들이 공유하는 bounded JSON/YAML graph parser다. 입력 text를 안전하게 parse하고
null-prototype object graph로 정규화하는 것까지만 담당하며, OpenAPI 의미 해석이나 요청 실행은
소비 앱이 소유한다.

## Parser 계약

`parseBoundedOpenApiDocument(text, format)`은 `json` 또는 `yaml` source를 다음 공통 상한으로
읽는다.

- source UTF-8 4 MiB, graph depth 40, node 50,000, string 16,384자, YAML alias 50개
- duplicate key와 `__proto__`/`prototype`/`constructor` dangerous key 거부
- cycle·alias가 확장된 unsupported/custom graph와 plain record가 아닌 객체 거부
- finite 값만 허용하고 JavaScript safe integer 범위를 벗어난 unsafe number 거부
- JSON comment/trailing comma와 YAML non-unique key/merge 확장을 허용하지 않음

파싱된 plain object는 `Object.create(null)` record로 복제하고, 배열과 scalar는 값의 형태를
유지한다. 상한·구문·graph 검증 실패는 raw parser text나 예외를 노출하지 않고 bounded error
code를 가진 `{ ok: false, error }` 결과로 반환한다. 이 package는 파일을 읽거나 URL을 fetch하지
않으며 `$ref`를 resolve하지 않는다.

## 소비자 경계

| 소비자 | package 밖에서 소유하는 동작 |
|---|---|
| API Playground | OpenAPI 3.0/3.1 version/root/path/server/parameter/security/body semantic validation, operation-to-request transformation, redaction과 draft/Collection 적용. URL source의 native fetch도 API Playground가 담당한다. |
| Webhook Lab | OpenAPI path/method/status를 Webhook `ResponseRule` preview로 projection하고 적용 여부를 결정한다. rule bounds와 unsafe path/operation 처리는 Webhook Lab이 담당한다. |

따라서 package는 endpoint URL을 조립하거나 요청을 보내지 않고, API Playground의 request draft나
Webhook Lab의 rule을 생성·저장하지 않는다. 각 앱은 parse 결과를 받은 뒤 자신의 semantic
validation과 bounded transformation을 다시 적용한다.

## 개발

```bash
pnpm --filter @devbox/openapi test
pnpm --filter @devbox/openapi build
```

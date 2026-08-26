# AppLink protocol v2 · one-time handoff implementation plan

> Issue: #284 · branch: `feat/crates/applink-handoff`

## Goal

`crates/applink`의 기존 path/profile/workspace/query argv 계약을 깨지 않으면서, 장문 text와
구조화 payload를 command line이나 clipboard에 노출하지 않는 protocol v2 handoff core를
제공한다. producer는 공용 data root에 완성된 bounded envelope을 발행하고 argv에는 kind와
opaque id만 넣는다. consumer는 target/kind/schema/expiry를 확인한 뒤 claim token을 얻고,
처리에 성공한 경우에만 ack한다.

## Scope boundary

포함:

- `OpenTarget::Handoff { kind, id }`, protocol version 2, argv/serde round trip
- 기존 v1 flag와 맨 앞 positional path parsing 호환
- 128-bit CSPRNG id와 claim token
- 10분 이하 TTL, serialized 10MiB 상한, bounded JSON parsing
- create-new publication과 exclusive claim record
- token 검증 ack, restore/nack, 60초 이하 lease renewal, lease-expiry crash recovery
- corrupt/expired cleanup, wrong target/kind 격리, concurrent consumer fixture
- secret reference·raw credential·payload path의 공통 fail-closed validation

제외:

- Webhook Lab/API Playground, Life Log/Knowledge 등 구체 producer/consumer UI
- 수신 앱 설치 확인, 실행/focus, clipboard fallback
- kind별 business schema와 import preview; 각 integration PR이 소유
- 10MiB 초과 binary 복제. 후속 producer는 검증된 app-owned/user-selected file reference만 사용

## Public contract

`HandoffEnvelope`은 `protocolVersion`, `id`, `kind`, `sourceApp`, optional `targetApp`,
`createdAtMs`, `expiresAtMs`, `payload`만 직렬화한다. kind는 `<lowercase-slug>/v<positive integer>`,
app id는 lowercase slug, id/token은 32자리 lowercase hex다. 오류는 fixed enum/message만 반환하고
payload, path, token 또는 parser 원문을 반영하지 않는다.

`OpenTarget::Handoff` JSON은 enum discriminator `kind: "handoff"`와 충돌하지 않게 payload kind를
`handoffKind`로 직렬화한다. argv pair는 `--handoff-kind`, `--handoff-id`이며 둘 중 하나만 있으면
parse error다. 구버전 parser가 모르는 flag를 무시하는 기존 동작은 유지한다.

## Storage model

공용 root의 versioned 위치는 `handoff/v1`이고 다음 두 directory만 만든다.

```text
handoff/v1/
  pending/<id>.json
  claimed/<id>.json
  claimed/<id>.lease.json
```

root는 absolute literal이어야 한다. root/home/current working directory 자체, filesystem root,
relative/`.`/`..`, symlink와 Windows reparse component를 거부한다. 생성한 directory도 즉시 다시
검사한다. managed slot은 regular file만 읽고 지우며 size metadata 뒤에도 `take(max + 1)`로
bounded read한다.

publication은 같은 directory의 mode 0600(unix) create-new temp에 전체 bytes를 write/flush/sync한
뒤 destination hard link를 create-new로 발행한다. 따라서 partial JSON은 관찰되지 않고 기존
destination을 덮어쓰지 않는다. 실패 시 자신의 nonce temp만 정리한다.

## State transitions

### Create

1. request/kind/app/TTL/payload를 I/O 전에 검증한다.
2. id를 생성하고 pending/claimed/lease 세 namespace 모두에서 충돌을 확인한다.
3. 완성 envelope을 10MiB 이하로 encode한다.
4. `pending/<id>.json`을 create-new로 발행한다. 충돌은 새 id로 최대 16회 재시도한다.

### Claim

1. existing claim을 읽어 active lease면 `AlreadyClaimed`를 반환한다.
2. expired lease면 기존 envelope을 pending에 create-new로 복구하고 old token record만 지운다.
3. pending을 bounded read하고 schema/id/kind/source/target/expiry/privacy를 다시 검증한다.
4. random token과 `min(now + 60s, expiresAt)` lease를 담은 claim record를 create-new로 발행한다.
5. pending을 제거한다. 제거 실패 시 자신의 claim record만 rollback한다.

동시 claimant는 같은 claimed destination을 create-new할 수 없으므로 하나만 성공한다.

### Ack / restore / renew

- ack는 disk record의 id/kind/target/consumer/token과 effective lease를 다시 확인하고 claimed를
  삭제한다. duplicate ack는 `Missing`이고 token이 바뀐 old consumer는 `TokenMismatch`다.
- restore는 original envelope을 pending에 create-new로 먼저 발행한 뒤 자신의 claimed를 지운다.
  이미 동일 envelope이 있으면 idempotent하게 이어가고 다른 bytes면 중단한다.
- renew는 같은 token만, 요청 60초 이하, payload expiry 이하에서 lease sidecar를 atomic replace한다.
  primary claim을 재확인해 concurrent ack가 끝난 payload를 sidecar가 되살리지 못하게 한다.
- corrupt/oversize/expired managed payload는 quarantine하거나 내용을 echo하지 않고 exact file만
  제거한다. link/reparse나 예상하지 않은 file type은 자동 삭제하지 않고 `UnsafeStorage`다.

## Privacy and path policy

- sensitive key(`authorization`, cookie, password, token, API/access/secret/private key 등)는 null,
  empty 또는 exact `${NAME}` reference만 허용한다.
- `{ name|key: "Authorization|X-Api-Key|...", value|content: ... }` row도 같은 규칙을 적용해
  generic `value` key 우회를 막는다.
- Bearer/Basic, `sk-`, private-key PEM, JWT-like triple, credential assignment/query와 URL userinfo를
  어느 string 위치에서든 거부한다.
- `path`, `filePath`, `sourcePath`, `binaryPath`, `tempPath`는 existing absolute path만 허용하고
  relative/dot component, filesystem root, symlink/reparse와 non-file/directory object를 거부한다.
  consumer는 실제 open 직전 kind schema와 ownership을 다시 검증해야 한다.
- nested depth 32, node 100,000, individual string 1MiB, serialized envelope 10MiB로 제한한다.

## Failure and durability rules

- public error text는 fixed and secret-free다.
- corrupt lease sidecar는 primary lease로 fail closed하고 sidecar만 정리한다.
- claim primary가 사라진 뒤 남은 lease는 orphan으로 정리한다.
- primary removal이 이미 성공한 ack/restore는 auxiliary lease cleanup 실패 때문에 payload를
  resurrect하거나 처리 결과를 뒤집지 않는다.
- directory sync는 Unix durability를 보강하는 best effort이며 atomic visibility 계약과 분리한다.

## Verification

Focused tests must cover argv/JSON compatibility, create/claim/ack, duplicate ack, wrong target/kind,
restore/token, lease expiry recovery/renewal cap, expired/corrupt cleanup, privacy/size/path rejection,
concurrent consumers and unsafe storage links. PR gate는 아래를 모두 수행한다.

```text
cargo test -p applink
cargo check -p applink
cargo clippy -p applink --all-targets -- -D warnings
cargo test --workspace
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
pnpm build
python3 .github/scripts/check-dependencies.py check
git diff --check
```

Windows W2에서는 packaged build, NTFS create-new/hard-link/replace, ACL, junction/reparse,
non-ASCII common root와 crash recovery를 별도 evidence로 남긴다.

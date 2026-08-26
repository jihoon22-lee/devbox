# Workthrough: AppLink protocol v2 · one-time handoff

## Summary

Issue #284의 공용 handoff core를 `crates/applink`에 구현했다. 기존 v1 target/positional path는
그대로 parsing하면서 protocol constant를 2로 올리고 `Handoff { kind, id }`를 추가했다. payload는
argv에 포함하지 않으며 공용 versioned store에서 token 기반으로 한 번만 claim/ack된다.

## Changes

### Protocol and public API

- `OpenTarget::Handoff`, `--handoff-kind`/`--handoff-id`, serde/argv round-trip
- enum tag와 payload kind 충돌을 피하는 `handoffKind` wire field
- `HandoffStore`, `CreateHandoff`, `HandoffEnvelope`, `HandoffDescriptor`, `HandoffClaim`
- fixed `HandoffError`, 10분 TTL, 60초 lease, 10MiB 상한 constants
- `handoff_root_in(common_root)`의 `handoff/v1` location helper

### Atomic state and recovery

- 128-bit CSPRNG lowercase hex id/token
- synced create-new temp + hard-link publication으로 incomplete/overwrite 차단
- claimed record의 exclusive publication으로 concurrent consumer 단일 승자 보장
- token/consumer/target/kind/schema/id/expiry를 매 operation마다 재검증
- ack의 one-time delete, restore의 pending-first rollback, expired lease crash recovery
- bounded renewal sidecar와 primary 재확인으로 concurrent ack 이후 resurrection 차단
- expired/corrupt exact managed file 및 orphan lease cleanup

검토 중 corrupt claimed record가 영구적으로 pending을 막지 않도록 exact claimed file을 정리하게
했고, ack/restore가 primary state를 이미 commit한 뒤 auxiliary lease cleanup 실패로 성공을 뒤집지
않게 했다. create id collision은 pending뿐 아니라 claimed/lease namespace 전체를 확인한다.

### Privacy and filesystem boundary

- raw Bearer/Basic/sk/private key/JWT/credential assignment/query/URL userinfo 차단
- sensitive direct key와 header-style name/value row 모두 exact secret reference만 허용
- JSON depth/node/string/serialized size bounds
- payload path는 existing absolute file/directory만 허용하고 relative, dot component, root,
  symlink/Windows reparse와 special file을 거부
- storage root와 모든 directory/managed slot도 link/reparse/unsafe root를 fail closed
- public error는 payload/token/path/parser text를 반영하지 않는 고정 문자열

## Dependency review

- `getrandom 0.4.3`: OS CSPRNG를 직접 사용해 id/token을 생성한다. permissive Apache-2.0 OR MIT.
- `serde_json 1`: envelope의 production encode/decode 때문에 dev dependency에서 direct dependency로
  이동했다. 기존 workspace lock에 이미 존재한다.
- local `filesystem`: renewal sidecar의 complete atomic replacement를 재사용한다.

`Cargo.lock`과 generated dependency notice는 최종 PR gate에서 다시 생성·검사한다. network runtime,
shell, registry 또는 외부 tool dependency는 추가하지 않았다.

## Review corrections

초기 focused implementation을 직접 검토하며 다음을 보정했다.

1. pending/claimed/lease 전 namespace collision 확인
2. corrupt claimed 및 invalid/orphan lease의 bounded cleanup
3. ack/restore primary commit 뒤 auxiliary cleanup의 harmless failure 처리
4. renewal의 payload expiry cap과 primary claim 재확인
5. generic `value` field를 통한 Authorization/API key 우회 차단
6. payload relative/symlink/reparse path 차단과 회귀 fixture

## Verification

최신 main `6ec5e7c` 위로 rebase했으며 유일한 content conflict인 generated
`THIRD_PARTY_NOTICES.md`는 결합된 lockfile에서 다시 생성했다. PR 직전 gate:

```text
cargo test -p applink -j2                                      PASS (57)
cargo check --workspace -j2                                    PASS
cargo clippy -p applink --all-targets -j2 -- -D warnings        PASS
cargo fmt --all -- --check                                     PASS
pnpm build                                                     PASS (17 workspace projects)
python3 .github/scripts/check-dependencies.py check             PASS
git diff --check                                                PASS
```

동일 tree의 full workspace Rust test/Clippy와 Windows compile/test는 GitHub Actions가 다시 수행한다.
로컬 Rust 명령은 `CARGO_INCREMENTAL=0`, single target, `-j2`로 제한했고 PR 검증 뒤 target cache를
즉시 정리한다.

## Deliberately skipped / remaining W2

- 특정 producer/consumer UI, launch/focus와 clipboard fallback
- kind별 import preview/business schema
- Windows packaged build와 NTFS/ACL/junction/reparse/non-ASCII/crash smoke
- lease renewal을 여러 thread가 같은 token으로 동시에 수행하는 것은 supported caller pattern이
  아니다. renewal은 monotonic하게 계산하고 ack와의 resurrection은 차단하지만, 같은 token의
  concurrent renewal serialization은 후속 OS-level lock 없이 보장하지 않는다.

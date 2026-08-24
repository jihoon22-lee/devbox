#!/usr/bin/env python3
"""release asset을 manifest와 대조 검증.

- manifest에 있는 모든 asset이 release에 존재하고 size·SHA-256이 일치하는지
- release에 있는 asset 중 manifest에 없는 것이 있는지 (manifest 자신은 예외)
실패 시 exit 1. 첫 구현에서는 자동 삭제하지 않고 실패만 보고한다 (부분 실패 시
복구를 어렵게 만들지 않기 위해, 문서 §17.3 PR 9).

용법: verify-release.py <owner/repo> <release-tag> <manifest-json> <gh-token>
"""
import hashlib
import json
import sys
import urllib.error
import urllib.request

API = "https://api.github.com"


def gh_get(url: str, token: str):
    req = urllib.request.Request(url, headers={
        "Authorization": f"Bearer {token}",
        "Accept": "application/vnd.github+json",
    })
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.load(resp)


def sha256_of(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> None:
    if len(sys.argv) != 5:
        raise SystemExit("usage: verify-release.py <owner/repo> <release-tag> <manifest-json> <gh-token>")
    repo, tag, manifest_path, token = sys.argv[1:]

    manifest = json.load(open(manifest_path))
    expected = {}
    for a in manifest["apps"]:
        for kind in ("portable", "installer"):
            expected[a["id"] + "/" + kind] = a[kind]
    if manifest.get("notices"):
        expected["notices"] = manifest["notices"]

    # release asset 목록 조회
    release = gh_get(f"{API}/repos/{repo}/releases/tags/{tag}", token)
    assets = release.get("assets", [])
    asset_map = {ast["name"]: ast for ast in assets}

    failures = []

    # 1. manifest의 모든 asset이 존재 + size/sha256 일치
    for key, info in expected.items():
        name = info["name"]
        ast = asset_map.get(name)
        if ast is None:
            failures.append(f"asset 없음: {name} ({key})")
            continue
        if ast["size"] != info["size"]:
            failures.append(f"size 불일치: {name} manifest={info['size']} actual={ast['size']}")
            continue
        url = ast["browser_download_url"]
        try:
            tmp = "/tmp/" + name.replace(" ", "_").replace("/", "_")
            urllib.request.urlretrieve(url, tmp)
        except urllib.error.URLError as e:
            failures.append(f"다운로드 실패: {name}: {e}")
            continue
        digest = sha256_of(tmp)
        if digest != info["sha256"]:
            failures.append(f"sha256 불일치: {name}")

    # 2. manifest에 없는 asset이 release에 있는지 (release-manifest.json 자신 제외)
    manifest_names = {i["name"] for i in expected.values()}
    for name in asset_map:
        if name == "release-manifest.json":
            continue
        if name not in manifest_names:
            failures.append(f"manifest에 없는 asset이 release에 있음: {name}")

    if failures:
        print("VERIFY FAILED:")
        for f in failures:
            print("  -", f)
        sys.exit(1)
    print(f"VERIFY OK: {len(expected)} assets 일치, 미선언 asset 없음")


if __name__ == "__main__":
    main()

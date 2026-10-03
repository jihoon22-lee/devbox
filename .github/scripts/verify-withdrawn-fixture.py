"""Pin the withdrawn public bytes as input, never as new acceptance evidence."""
import hashlib
from pathlib import Path
import sys
from suite_release_contract import verify_public_assets

SOURCE = "e499ac7127269bf67863bf0fdc42eaf53236b9f3"
MANIFEST_SHA256 = "27a3dd7b2dab585fa6a930d3a1e64bd4f1f3166a141c926080f005bc16114b4b"


def verify(directory):
    manifest = directory / "release-manifest.json"
    if manifest.is_symlink() or hashlib.sha256(manifest.read_bytes()).hexdigest() != MANIFEST_SHA256:
        raise ValueError("withdrawn manifest identity changed")
    verified = verify_public_assets(directory, source=SOURCE)
    if verified["releaseTag"] != "v0.9.0" or verified["suiteVersion"] != "0.9.0":
        raise ValueError("withdrawn fixture version changed")
    return verified


if __name__ == "__main__":
    verify(Path(sys.argv[1]))
    print("Pinned withdrawn public package bytes verified; this is fixture input only.")

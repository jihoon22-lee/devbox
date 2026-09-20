"""Closed v0.8 public package topology, shared by assembly and read-back.

Legacy release parsers remain available separately for pinned migration evidence.
This module never extracts an archive or executes a downloaded file.
"""
import hashlib
import json
import re
import stat
import zipfile
from pathlib import Path

PRODUCTS = ("workspace", "api-studio", "knowledge", "control-center")
MAX_BYTES = 1024 * 1024 * 1024
SHA = re.compile(r"[a-f0-9]{64}")
VERSION = re.compile(r"(?:0|[1-9][0-9]*)(?:\.(?:0|[1-9][0-9]*)){2}")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def file_names(product):
    names = {f"devbox-{product}.exe", "THIRD_PARTY_NOTICES.md", "devbox-installation.json"}
    if product == "workspace":
        names |= {"resources/wsl/manifest.json", "resources/wsl/devbox-workspace-wsl"}
    if product == "control-center":
        names.add("resources/suite/devbox-suite-bootstrap.exe")
    return names


def asset(value, name):
    require(isinstance(value, dict) and set(value) == {"name", "size", "sha256"}, "invalid asset shape")
    require(value["name"] == name, "unexpected asset name")
    require(type(value["size"]) is int and 0 < value["size"] <= MAX_BYTES, "invalid asset size")
    require(isinstance(value["sha256"], str) and SHA.fullmatch(value["sha256"]), "invalid asset digest")
    return value


def manifest_assets(manifest, tag=None, commit=None, allow_prerelease=False):
    require(isinstance(manifest, dict) and set(manifest) == {"schemaVersion", "releaseTag", "sourceSha", "suiteVersion", "protocolVersion", "setup", "products", "notices"}, "invalid Suite manifest envelope")
    require(manifest["schemaVersion"] == 2 and manifest["protocolVersion"] == 1, "unsupported Suite manifest protocol")
    version = manifest["suiteVersion"]
    require(isinstance(version, str) and VERSION.fullmatch(version), "invalid Suite version")
    release_tag = manifest["releaseTag"]
    valid_tag = release_tag == f"v{version}" or (allow_prerelease and isinstance(release_tag, str) and re.fullmatch(re.escape(f"v{version}")+r"-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*", release_tag))
    require(valid_tag and (tag is None or tag == release_tag), "Suite tag/version mismatch")
    source = manifest["sourceSha"]
    require(isinstance(source, str) and re.fullmatch(r"[a-f0-9]{40}", source) and (commit is None or source == commit), "Suite source mismatch")
    require(isinstance(manifest["products"], list) and len(manifest["products"]) == 4, "Suite must have four products")
    require([p.get("id") for p in manifest["products"] if isinstance(p, dict)] == list(PRODUCTS), "Suite product identities/order mismatch")
    expected = {}
    for name, key in [(f"Devbox_{version}_x64-setup.exe", "setup"), ("THIRD_PARTY_NOTICES.md", "notices")]:
        expected[name] = asset(manifest[key], name)
    for product in manifest["products"]:
        require(set(product) == {"id", "version", "portable", "files"} and product["version"] == version, "Suite product shape/version mismatch")
        pid = product["id"]
        name = f"devbox-{pid}_{version}_x64.zip"
        expected[name] = asset(product["portable"], name)
        files = product["files"]
        require(isinstance(files, list) and len(files) == len(file_names(pid)), "product file count mismatch")
        require({f.get("name") for f in files if isinstance(f, dict)} == file_names(pid), "product component/file closure mismatch")
        for member in files:
            asset(member, member["name"])
            if member["name"] == "THIRD_PARTY_NOTICES.md":
                require(member == manifest["notices"], "product notices differ from public notices")
        require(sum(f["size"] for f in files) <= MAX_BYTES, "product unpacked size limit")
    require(len(expected) == 6, "Suite declared asset count mismatch")
    return expected


def digest_stream(stream):
    size, digest = 0, hashlib.sha256()
    while chunk := stream.read(65536):
        size += len(chunk)
        require(size <= MAX_BYTES, "asset exceeds size limit")
        digest.update(chunk)
    return size, digest.hexdigest()


def verify_archive(directory, product, manifest):
    path = Path(directory) / product["portable"]["name"]
    require(path.is_file() and not path.is_symlink(), "portable must be a regular file")
    with zipfile.ZipFile(path) as archive:
        entries = archive.infolist()
        names = [entry.filename for entry in entries]
        require(len(names) == len(set(name.casefold() for name in names)) and set(names) == file_names(product["id"]), "ZIP file closure or duplicate mismatch")
        declared = {member["name"]: member for member in product["files"]}
        for entry in entries:
            require(not entry.is_dir() and not entry.flag_bits & 1, "ZIP directories/encryption are not allowed")
            kind = stat.S_IFMT(entry.external_attr >> 16)
            require(kind in (0, stat.S_IFREG), "ZIP links/special files are not allowed")
            require(entry.compress_type in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED), "unsupported ZIP compression")
            member = declared[entry.filename]
            require(entry.file_size == member["size"], "ZIP member size mismatch")
            with archive.open(entry) as stream:
                size, digest = digest_stream(stream)
            require(size == member["size"] and digest == member["sha256"], "ZIP member digest mismatch")
        entry = declared["devbox-installation.json"]
        require(entry["size"] <= 65536, "portable manifest exceeds limit")
        portable = json.loads(archive.read("devbox-installation.json"))
        exe = declared[f"devbox-{product['id']}.exe"]
        require(portable == {"schemaVersion": 1, "installationId": "portable", "generation": f"portable-{manifest['sourceSha']}", "suiteVersion": manifest["suiteVersion"], "protocolVersion": 1, "members": [{"product": product["id"], "executable": exe["name"], "sha256": exe["sha256"]}]}, "portable installation identity mismatch")


def components(manifest):
    return [{"owner": product["id"], **member} for product in manifest["products"] for member in product["files"] if member["name"].startswith("resources/")]


def verify_public_assets(directory, source=None, tag=None):
    directory = Path(directory)
    manifest_path = directory / "release-manifest.json"
    require(manifest_path.is_file() and not manifest_path.is_symlink(), "linked/missing public manifest")
    require(manifest_path.stat().st_size <= 1024 * 1024, "manifest exceeds limit")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    expected = manifest_assets(manifest, tag=tag, commit=source)
    require({p.name for p in directory.iterdir()} == set(expected) | {"release-manifest.json"}, "public asset set mismatch")
    for name, identity in expected.items():
        path = directory / name
        require(path.is_file() and not path.is_symlink(), "linked/missing public asset")
        with path.open("rb") as stream:
            size, digest = digest_stream(stream)
        require(size == identity["size"] and digest == identity["sha256"], "public asset changed")
    for product in manifest["products"]:
        verify_archive(directory, product, manifest)
    return manifest


def acceptance_config(manifest):
    identities = {
        "workspace": ("workspace", "overview"),
        "api-studio": ("apistudio", "requests"),
        "knowledge": ("knowledge", "notes"),
        "control-center": ("controlcenter", "products"),
    }
    return {"schemaVersion": 2, "suiteVersion": manifest["suiteVersion"], "products": [
        {"id": p, "version": manifest["suiteVersion"], "identifier": f"com.devbox.v08.{identities[p][0]}",
         "executable": f"devbox-{p}.exe", "defaultRoute": identities[p][1]} for p in PRODUCTS
    ]}

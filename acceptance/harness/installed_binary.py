"""Validate the actual deploy.rs contract and keep execution pinned to its file."""
from contextlib import contextmanager
import hashlib
import os
from pathlib import Path
import re
import stat


def digest_fd(fd):
    os.lseek(fd,0,os.SEEK_SET);digest=hashlib.sha256()
    while chunk:=os.read(fd,1024*1024):digest.update(chunk)
    os.lseek(fd,0,os.SEEK_SET)
    return digest.hexdigest()


@contextmanager
def installed_binary(binary, expected_sha, *, home, before_open=lambda:None):
    binary=Path(binary);home=Path(home)
    if binary!=home/'.local/share/devbox-accept/bin/devbox' or not binary.is_absolute() or not re.fullmatch('[0-9a-f]{64}',expected_sha):
        raise ValueError('Exact installed accept path and Linux artifact hash required')
    descriptors=[]
    try:
        fd=os.open('/',os.O_RDONLY|os.O_DIRECTORY);descriptors.append(fd)
        for part in binary.parent.parts[1:]:
            fd=os.open(part,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=fd);descriptors.append(fd)
        directory=fd;meta=os.fstat(directory)
        if meta.st_uid!=os.getuid() or stat.S_IMODE(meta.st_mode)!=0o700:
            raise ValueError('Foreign or nonprivate installed bin directory')
        owner=os.open('.owner',os.O_RDONLY|os.O_NOFOLLOW,dir_fd=directory);descriptors.append(owner)
        if not stat.S_ISREG(os.fstat(owner).st_mode) or os.fstat(owner).st_uid!=os.getuid() or os.read(owner,128)!=b'devbox-desktop-bin-v1\n':
            raise ValueError('Installed bin owner marker mismatch')
        target=os.readlink('devbox',dir_fd=directory)
        if target!='devbox-'+expected_sha:
            raise ValueError('Installed link differs from exact Linux artifact')
        before_open()
        file=os.open(target,os.O_RDONLY|os.O_NOFOLLOW,dir_fd=directory);descriptors.append(file)
        meta=os.fstat(file)
        if not stat.S_ISREG(meta.st_mode) or meta.st_uid!=os.getuid() or not meta.st_mode&stat.S_IXUSR or meta.st_mode&0o022:
            raise ValueError('Installed target ownership/type/mode mismatch')
        if digest_fd(file)!=expected_sha:
            raise ValueError('Installed target differs from Linux artifact SHA256')
        # A replaced path never redirects execution: child inherits this exact fd.
        yield file, str(binary.parent/target)
    finally:
        for fd in reversed(descriptors):os.close(fd)


def verify_linux_artifact(directory, source_sha, expected_version):
    """The exact three-file windows.yml linux-bin artifact, without executing it."""
    import json
    directory=Path(directory)
    names={'devbox-linux','linux-receipt.json','devbox-version.txt'}
    if directory.is_symlink() or not directory.is_dir() or {p.name for p in directory.iterdir()}!=names:
        raise ValueError('Unexpected Linux artifact entries')
    if any((directory/name).is_symlink() or not (directory/name).is_file() for name in names):
        raise ValueError('Linux artifact entries must be regular files')
    proof=json.loads((directory/'linux-receipt.json').read_text())
    if proof.get('source_sha')!=source_sha or not re.fullmatch('[0-9a-f]{64}',proof.get('sha256','')):
        raise ValueError('Linux artifact source/hash receipt mismatch')
    if (directory/'devbox-version.txt').read_text()!=expected_version:
        raise ValueError('Linux artifact version mismatch')
    if hashlib.sha256((directory/'devbox-linux').read_bytes()).hexdigest()!=proof['sha256']:
        raise ValueError('Linux artifact binary hash mismatch')
    return {**proof,'version':expected_version}

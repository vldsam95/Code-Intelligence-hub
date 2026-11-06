from __future__ import annotations

import os
import pwd
import shutil
import stat
import sys
from pathlib import Path


def prepare_master_key() -> None:
    source = Path("/run/secrets-root/hub-master.key")
    destination = Path("/run/codeintel-secrets/hub-master.key")
    if not source.is_file():
        raise SystemExit(f"Required master key is missing: {source}")

    account = pwd.getpwnam("codeintel")
    destination.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    shutil.copyfile(source, destination)
    destination.chmod(stat.S_IRUSR)
    os.chown(destination, account.pw_uid, account.pw_gid)
    os.chown(destination.parent, account.pw_uid, account.pw_gid)
    os.environ["CODEINTEL_MASTER_KEY_FILE"] = str(destination)


def drop_privileges_and_exec(command: list[str]) -> None:
    account = pwd.getpwnam("codeintel")
    os.setgroups([])
    os.setgid(account.pw_gid)
    os.setuid(account.pw_uid)
    os.execvpe(command[0], command, os.environ)


if __name__ == "__main__":
    if len(sys.argv) < 2:
        raise SystemExit("No Hub command was supplied")
    prepare_master_key()
    drop_privileges_and_exec(sys.argv[1:])

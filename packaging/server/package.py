#!/usr/bin/env python3
"""Package an already-built headless binary and complete static UI; no GUI runtime."""
import argparse
import hashlib
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument('--binary', type=Path, required=True)
parser.add_argument('--ui', type=Path, required=True)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--version', required=True)
args = parser.parse_args()
assert args.binary.is_file() and (args.ui / 'index.html').is_file()
assert __import__('re').fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', args.version)
arch = platform.machine()
deb_arch = {'x86_64': 'amd64', 'aarch64': 'arm64'}[arch]
args.output.mkdir(parents=True, exist_ok=True)
source = Path(__file__).resolve().parent
name = f'harness-server-{args.version}-linux-{arch}'
with tempfile.TemporaryDirectory(prefix='harness-package-') as temp:
    root = Path(temp)
    payload = root / 'usr/lib/harness'
    payload.mkdir(parents=True)
    shutil.copy2(args.binary, payload / 'harness')
    (payload / 'harness').chmod(0o755)
    shutil.copytree(args.ui, payload / 'web')
    shutil.copy2(source / 'server-control', payload / 'server-control')
    (payload / 'server-control').chmod(0o755)
    unit = root / 'usr/lib/systemd/system'
    unit.mkdir(parents=True)
    shutil.copy2(source / 'harness-server.service', unit)
    sudoers = root / 'etc/sudoers.d'
    sudoers.mkdir(parents=True)
    (sudoers / 'harness-server').write_text('harness ALL=(root) NOPASSWD: /usr/lib/harness/server-control *\n')
    (sudoers / 'harness-server').chmod(0o440)
    licenses = root / 'usr/share/doc/harness-server'
    licenses.mkdir(parents=True)
    shutil.copy2(source.parents[1] / 'LICENSE', licenses / 'copyright')
    # The tarball also has an installer for systemd distributions without dpkg.
    shutil.copy2(source / 'install-tar.sh', root / 'install.sh')
    shutil.copy2(source / 'postinst', root / 'configure.sh')
    with tarfile.open(args.output / f'{name}.tar.gz', 'w:gz') as tar:
        for item in sorted(root.iterdir()):
            tar.add(item, arcname=item.name)
    (root / 'install.sh').unlink()
    (root / 'configure.sh').unlink()
    control = root / 'DEBIAN'
    control.mkdir()
    (control / 'control').write_text(f'Package: harness-server\nVersion: {args.version}\nArchitecture: {deb_arch}\nMaintainer: StrukLabs\nDepends: libc6, libgcc-s1, libstdc++6, libssl3, libdbus-1-3, ca-certificates, python3, sudo, adduser, systemd\nDescription: Harness headless server and browser administration\n')
    (control / 'conffiles').write_text('/etc/sudoers.d/harness-server\n')
    for script in ('postinst', 'prerm', 'postrm'):
        shutil.copy2(source / script, control / script)
        (control / script).chmod(0o755)
    subprocess.run(['dpkg-deb', '--root-owner-group', '--build', str(root), str(args.output / f'{name}.deb')], check=True)
for suffix in ('tar.gz', 'deb'):
    artifact = args.output / f'{name}.{suffix}'
    artifact.with_name(artifact.name + '.sha256').write_text(hashlib.sha256(artifact.read_bytes()).hexdigest() + '  ' + artifact.name + '\n')

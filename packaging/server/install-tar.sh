#!/bin/sh
# Run inside an extracted, trusted headless tarball. Requires systemd, sudo, Python 3 and adduser.
set -eu
[ "$(id -u)" = 0 ] || exit 1
cd "$(dirname "$0")"
command -v systemctl >/dev/null
command -v sudo >/dev/null
command -v python3 >/dev/null
cp -R usr/lib/harness /usr/lib/
install -m 0644 usr/lib/systemd/system/harness-server.service /usr/lib/systemd/system/
install -m 0440 etc/sudoers.d/harness-server /etc/sudoers.d/
sh ./configure.sh configure

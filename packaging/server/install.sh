#!/bin/sh
# Usage: curl -fsSL <raw URL>/packaging/server/install.sh | sudo sh
# Or: sudo sh install.sh ./harness-server-<version>-linux-<arch>.deb
set -eu
[ "$(id -u)" = 0 ] || { echo 'Run as root (sudo sh install.sh).' >&2; exit 1; }
command -v dpkg >/dev/null || { echo 'This installer requires Debian/Ubuntu. Use the tar archive elsewhere.' >&2; exit 1; }
if [ "$#" = 1 ]; then
  exec dpkg -i "$1"
fi
[ "$#" = 0 ] || exit 2
command -v python3 >/dev/null
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT HUP INT TERM
python3 - "$work/server.deb" <<'PY'
import hashlib, json, os, re, sys, urllib.request
def fetch(url, limit):
    with urllib.request.urlopen(urllib.request.Request(url, headers={'User-Agent':'HarnessStudio'}), timeout=60) as r:
        data = r.read(limit + 1)
    if len(data) > limit: raise SystemExit('Download too large')
    return data
release = json.loads(fetch('https://api.github.com/repos/struklabs/harness-studio/releases/latest', 4*1024*1024))
tag = release['tag_name']
if release['draft'] or release['prerelease'] or not re.fullmatch(r'v[0-9]+\.[0-9]+\.[0-9]+', tag): raise SystemExit('Stable release required')
arch = os.uname().machine
if arch not in ('x86_64', 'aarch64'): raise SystemExit('Unsupported architecture')
name = f'harness-server-{tag[1:]}-linux-{arch}.deb'
asset = next((a for a in release['assets'] if a['name'] == name), None)
if not asset: raise SystemExit('This release has no headless server package. Build from source; see the server guide.')
url = f'https://github.com/struklabs/harness-studio/releases/download/{tag}/{name}'
if asset['browser_download_url'] != url or not re.fullmatch(r'sha256:[0-9a-f]{64}', asset.get('digest') or ''): raise SystemExit('Official SHA-256 digest required')
data = fetch(url, 256*1024*1024)
if 'sha256:'+hashlib.sha256(data).hexdigest() != asset['digest']: raise SystemExit('Digest mismatch')
with open(sys.argv[1], 'wb') as f: f.write(data)
PY
dpkg -i "$work/server.deb"

#!/bin/sh
set -eu
test "$(id -u)" = 0 || { echo 'Run installer as root'; exit 1; }
python3 -c 'import sys; assert sys.version_info >= (3,10), "Python 3.10+ required"'
command -v systemctl >/dev/null
if ! id worldcompass >/dev/null 2>&1; then
    useradd --system --home-dir /var/lib/world-compass-collector --shell /usr/sbin/nologin worldcompass
fi
install -d -m 755 /opt/world-compass-collector
install -d -o worldcompass -g worldcompass -m 700 /var/lib/world-compass-collector
install -m 644 collector.py japanese-collection-plan.json /opt/world-compass-collector/
install -m 644 world-compass-collector.service /etc/systemd/system/
systemctl daemon-reload
echo 'Installed. Import the public snapshot and configure contact before interactive VRChat login.'
echo 'The collector has NOT been started or enabled yet.'

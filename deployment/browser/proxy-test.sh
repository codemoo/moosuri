#!/bin/sh
set -eu
# Synthetic public IP exists only on this test container's loopback interface.
# Production public_connect uses its normal DNS/IP policy, with no test bypass.
ip address add 93.184.216.34/32 dev lo
python3 /capsule/fixture.py &
exec setpriv --reuid=10001 --regid=10001 --clear-groups --no-new-privs --inh-caps=-all --ambient-caps=-all --bounding-set=-all /usr/local/bin/browser-proxy

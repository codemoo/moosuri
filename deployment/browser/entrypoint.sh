#!/bin/sh
set -eu
# The validation path accepts only canonical RFC1918 IPv4 and fixed port 3128.
# It performs no network access. Configuration failure is fatal before launch.
: "${MOOSURI_BROWSER_PROXY_IP:?configure the private IPv4 proxy address}"
proxy=$(/usr/local/bin/browser-worker --validate-proxy)
[ "$proxy" = "$MOOSURI_BROWSER_PROXY_IP:3128" ] || exit 1
# All rules below apply only inside this dedicated container namespace.
iptables -w -P OUTPUT DROP
iptables -w -P INPUT DROP
iptables -w -P FORWARD DROP
iptables -w -A OUTPUT -o lo -j ACCEPT
iptables -w -A INPUT -i lo -j ACCEPT
iptables -w -A OUTPUT -p tcp -d "$MOOSURI_BROWSER_PROXY_IP" --dport 3128 -j ACCEPT
iptables -w -A INPUT -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT
ip6tables -w -P OUTPUT DROP
ip6tables -w -P INPUT DROP
ip6tables -w -P FORWARD DROP
ip6tables -w -A OUTPUT -o lo -j ACCEPT
ip6tables -w -A INPUT -i lo -j ACCEPT
printf 'moosuri-browser-isolation-v2\nproxy=%s\n' "$proxy" > /run/browser-isolation
chmod 0444 /run/browser-isolation
exec setpriv --reuid=10001 --regid=10001 --clear-groups --no-new-privs --inh-caps=-all --ambient-caps=-all --bounding-set=-all /usr/local/bin/browser-worker "$@"

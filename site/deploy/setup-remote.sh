#!/bin/sh
# One-time setup on the server: TLS certificate (Let's Encrypt over Cloudflare DNS,
# like the other *.bharathi.fyi sites), then a proxied DNS record. Idempotent.
# Runs on the VM; reads CLOUDFLARE_API_TOKEN / CLOUDFLARE_ZONE_ID from ~/.bashrc.
set -eu
HOST=docforge.bharathi.fyi
# The DNS record points at this server: its public IPv4 address.
IP=$(ip -4 route get 1.1.1.1 | sed -n "s/.* src \([0-9.]*\).*/\1/p")
[ -n "$IP" ] || { echo "setup: could not find this server's IPv4 address" >&2; exit 1; }
eval "$(grep -E '^export CLOUDFLARE_(API_TOKEN|ZONE_ID)=' /root/.bashrc)"

if [ ! -f "/etc/letsencrypt/live/$HOST/fullchain.pem" ]; then
  certbot certonly --non-interactive --agree-tos --keep-until-expiring \
    --dns-cloudflare --dns-cloudflare-credentials /root/.secrets/cloudflare-gmb.ini \
    --dns-cloudflare-propagation-seconds 30 -d "$HOST" >/dev/null
  echo "certificate: issued"
else
  echo "certificate: present"
fi

api="https://api.cloudflare.com/client/v4/zones/$CLOUDFLARE_ZONE_ID/dns_records"
count=$(curl -fsS -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" "$api?type=A&name=$HOST" | python3 -c 'import sys,json;print(len(json.load(sys.stdin)["result"]))')
if [ "$count" = 0 ]; then
  curl -fsS -X POST -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" -H "Content-Type: application/json" "$api" \
    --data "{\"type\":\"A\",\"name\":\"$HOST\",\"content\":\"$IP\",\"proxied\":true,\"ttl\":1,\"comment\":\"Doc Forge landing page\"}" \
    | python3 -c 'import sys,json;d=json.load(sys.stdin);print("dns: created" if d["success"] else d["errors"])'
else
  echo "dns: present"
fi

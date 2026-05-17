#!/bin/bash
set -e

mkdir -p /root/.lightning

cat > /root/.lightning/config <<EOF
network=regtest
bitcoin-rpcuser=user
bitcoin-rpcpassword=pass
bitcoin-rpchost=${BITCOIN_RPC_HOST}
bitcoin-rpcport=18443
log-level=debug
alias=${NODE_ALIAS}
plugin=${PLUGIN_PATH}
EOF

chmod +x ${PLUGIN_PATH}

exec lightningd
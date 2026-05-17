#!/bin/bash
set -ex

COMPOSE="docker compose -f cln-docker/docker-compose.yml"
CLI1="$COMPOSE exec cln1 lightning-cli --network=regtest"
CLI2="$COMPOSE exec cln2 lightning-cli --network=regtest"
CLI3="$COMPOSE exec cln3 lightning-cli --network=regtest"
BCLI="$COMPOSE exec bitcoind bitcoin-cli -regtest -rpcuser=user -rpcpassword=pass"

echo "==> Building plugin..."
cargo build

echo "==> Starting containers..."
$COMPOSE up -d --build

echo "==> Waiting for nodes to start..."
sleep 10

echo "==> Creating bitcoind wallet..."
set +e
$BCLI createwallet "default"
set -e
sleep 2

echo "==> Mining initial blocks..."
ADDR=$($BCLI getnewaddress)
$BCLI generatetoaddress 110 "$ADDR" > /dev/null

echo "==> Funding CLN nodes..."
ADDR1=$($CLI1 newaddr | jq -r '.bech32')
ADDR2=$($CLI2 newaddr | jq -r '.bech32')
ADDR3=$($CLI3 newaddr | jq -r '.bech32')
$BCLI sendtoaddress "$ADDR1" 1
$BCLI sendtoaddress "$ADDR2" 1
$BCLI sendtoaddress "$ADDR3" 1
$BCLI generatetoaddress 6 "$ADDR" > /dev/null
sleep 5

echo "==> Connecting nodes: alice -> bob -> carol..."
BOB_ID=$($CLI2 getinfo | jq -r '.id')
CAROL_ID=$($CLI3 getinfo | jq -r '.id')
BOB_ADDR="$BOB_ID@cln2:9735"
CAROL_ADDR="$CAROL_ID@cln3:9735"

$CLI1 connect "$BOB_ADDR"
$CLI2 connect "$CAROL_ADDR"

echo "==> Opening channels..."
$CLI1 fundchannel "$BOB_ID" 500000
$CLI2 fundchannel "$CAROL_ID" 500000
$BCLI generatetoaddress 6 "$ADDR" > /dev/null
sleep 10

echo "==> Creating BOLT12 offers for bob and carol..."
BOB_OFFER=$($CLI2 offer any "bob's offer" | jq -r '.bolt12')
CAROL_OFFER=$($CLI3 offer any "carol's offer" | jq -r '.bolt12')

echo "  bob offer:   $BOB_OFFER"
echo "  carol offer: $CAROL_OFFER"

echo "==> Creating prism on alice..."
PRISM=$($CLI1 prism-create \
  description="test prism" \
  outlay_factor=1.0 \
  members="[
    {\"description\":\"Bob\",\"destination\":\"$BOB_OFFER\",\"split\":1.0,\"fees_incurred_by\":\"local\",\"payout_threshold_msat\":0},
    {\"description\":\"Carol\",\"destination\":\"$CAROL_OFFER\",\"split\":1.0,\"fees_incurred_by\":\"local\",\"payout_threshold_msat\":0}
  ]")

echo "$PRISM" | jq .
PRISM_ID=$(echo "$PRISM" | jq -r '.prism_id')
echo "  prism_id: $PRISM_ID"

echo "==> Listing prisms..."
$CLI1 prism-list | jq .

echo "==> Executing prism-pay..."
$CLI1 prism-pay prism_id="$PRISM_ID" amount_msat=100000 | jq .

echo "==> Creating a binding..."
ALICE_OFFER=$($CLI1 offer any "alice's offer" | jq -r '.offer_id')
$CLI1 prism-addbinding prism_id="$PRISM_ID" offer_id="$ALICE_OFFER" | jq .

echo "==> Listing bindings..."
$CLI1 prism-listbindings | jq .

echo ""
echo "✓ All tests passed."
echo ""
echo "To explore manually:"
echo "  docker compose -f cln-docker/docker-compose.yml exec cln1 lightning-cli --network=regtest getinfo"
echo ""
echo "To tear down:"
echo "  docker compose -f cln-docker/docker-compose.yml down -v"
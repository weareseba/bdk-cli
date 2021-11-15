#/bin/sh

cargo run --features "compact_filters" -- \
    -n testnet \
    wallet \
    --node 127.0.0.1:18334 \
    --descriptor "wpkh(cVpPVruEDdmutPzisEsYvtST1usBR3ntr8pXSyt6D2YYqXRyPcFW)" \
    sync

rm -rf ~/.bdk-bitcoin/compact_filters

cargo run --features "compact_filters" -- \
    -n testnet \
    wallet \
    --node 127.0.0.1:18334 \
    --descriptor "wpkh(cVpPVruEDdmutPzisEsYvtST1usBR3ntr8pXSyt6D2YYqXRyPcFW)" \
    sync

cargo run --features "compact_filters" -- \
    -n testnet \
    wallet \
    --node 127.0.0.1:18334 \
    --descriptor "wpkh(cVpPVruEDdmutPzisEsYvtST1usBR3ntr8pXSyt6D2YYqXRyPcFW)" \
    sync

cargo run --features "compact_filters" -- \
    -n testnet \
    wallet \
    --node 127.0.0.1:18334 \
    --descriptor "wpkh(cVpPVruEDdmutPzisEsYvtST1usBR3ntr8pXSyt6D2YYqXRyPcFW)" \
    sync

cargo run --features "compact_filters" -- \
    -n testnet \
    wallet \
    --node 127.0.0.1:18334 \
    --descriptor "wpkh(cVpPVruEDdmutPzisEsYvtST1usBR3ntr8pXSyt6D2YYqXRyPcFW)" \
    sync

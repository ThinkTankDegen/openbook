For the original README, check the GigaDAO version [here](https://github.com/GigaDAO/openbook)

## Installation
`cargo build --release`

## Environment preparation
1.  Set the `KEY_PATH` environment variable to point to your wallet's keypair json file.
```
export KEY_PATH=/PATH/TO/YOUR/keypair.json
```
2.  Set the `OOS_KEY` environment variable to the open orders account for the market you want to act on.
```
export OOS_KEY=TheOPENordersACCOUNTofYOURwalletFORtheMARKETyouWANTtoACTon
```

## Run the command to get the market info
```
./target/release/openbook-v1-cli \                         
  --market-id TheMARKETid \
  info
```
If you do have open orders on that market for that wallet and that open orders account you should see a non-empty `open_asks` or `open_bids` in your output. E.g.:
```
2025-11-11T00:35:38.725831Z  INFO [*] OB_V1_Client:
OB_V1_Client {
    ...
    open_orders: OpenOrders {
        ...
        open_asks: [3486434629931105260441, 7378697629483820651393, 14572927818230545781413, 15661285718579409326369, 16417602225601500943015, 17505960125950364488358, 18262276632972456104616]
        open_bids: []
        bids_address: AGvgip8dLAtufAgq8mYU3gWok92ccFHNsQdKjpgv5QT3
        asks_address: EBRdhpJWNSZYALtcVnhrvL4XXbSDb6uNkxAUW6XDy1bF
        open_asks_prices: [0.0189, 0.04, 0.079, 0.0849, 0.089, 0.0949, 0.099]
        open_bids_prices: []
        base_total: 0.0
        quote_total: 0.0
    }
...
```

## Cancel open orders (Simulation mode)
Run this command to simulate the cancel of up to `MAX_CANCEL_ORDERS` at once. This is already set to 5 in the code in order to avoid exceeding the block limit. Feel free to change it. If you have more open orders you can re-run the command.
```
./target/release/openbook-v1-cli \
  --market-id TheMARKETid \
  cancel
```

## Cancel open orders (Actual execution mode)
Run this command to cancel up to `MAX_CANCEL_ORDERS` at once. This is already set to 5 in the code in order to avoid exceeding the block limit. Feel free to change it. If you have more open orders you can re-run the command.
```
./target/release/openbook-v1-cli \
  --market-id TheMARKETid \
  cancel -e
```

## Settle funds (Simulation mode)
Run this command to simulate the settlement of funds to your wallet.
```
./target/release/openbook-v1-cli \
  --market-id TheMARKETid \
  settle
```


## Settle funds (Actual execution mode)
Run this command to settle funds to your wallet.
```
./target/release/openbook-v1-cli \
  --market-id TheMARKETid \
  settle -e
```

Any questions? Ping me:
- Here
- On X - https://x.com/ThinkTank_X
- On Discord - think.tank.
- Degen wallet: BqpVsNuznx4tJPNC4RNd2FmoFzMu4wPbQMMwEHZiyV6B

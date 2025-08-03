# FRAME Pallet for HTLC

The current repo contains a [FRAME pallet](https://paritytech.github.io/polkadot-sdk/master/polkadot_sdk_docs/polkadot_sdk/frame_runtime/index.html),
named `pallet-htlc`, which implements functionality to create
[hashed timelock contracts (HTLC) which can be used for asset swaps](https://1inch.io/assets/1inch-fusion-plus.pdf).

The Substrate pallet, follows the logic of the [`cross-chain-swap`](https://github.com/1inch/cross-chain-swap)
smart contracts by 1inch. The core logic of the pallet handles both `EscrowSrc`
and `EscrowDst` contracts as well as ways to withdraw assets from them and
cancel them.

1inch Fusion+ cross chain swaps utilize both the `cross-chain-swap` smart
contracts and the [`limit-order-protocol`](https://github.com/1inch/limit-order-protocol/tree/master/contracts) contracts.

A maker, an actor with the intention to swap `AssetX:AmountX` for `AssetY:AmountY`,
submits an limit order, by calling the `limit-order-protocol` contract.
A relayer (currently run by 1inch) runs a Dutch auction to find resolvers (takers)
who want to facilitate the asset exchange.

`pallet-htlc` currently implements only a simple and naive version of the intent
of a maker to swap `AssetX:AmountX` for `AssetY:AmountY`.

The repo is based on the [minimal template for creating a blockchain based on Polkadot SDK](https://github.com/paritytech/polkadot-sdk-minimal-template)
and FRAME pallet repos used for educational purposes by
[Polkadot Blockchain Academy](https://github.com/Polkadot-Blockchain-Academy).

## Build & run the FRAME pallet

To run the pallet and the tests which simulate various use cases for which
the FRAME pallet can be used:

```bash
cargo build
cargo test
```

After making any code modifications, format the code with `cargo +nightly fmt`.

## Implementation details

The pallet implements two calls that can be used by a resolver to deploy
a HTLC and store its data on-chain:

- `create_src_htlc`: HTLC for when the maker intents to **send** assets from the current chain. This creates a HTLC with `HtlcType::Source`.
- `create_dst_htlc`: HTLC for when the maker intents to **receive** assets to the current chain. This creates a HTLC with `HtlcType::Destination`.

The other HTLC-related calls that the pallet are:
- `withdraw`: funds are send to the recipient; only the resolver who created the HTLC can call.
- `public_withdraw`: funds are send to the recipient; any resolver can call.
- `cancel`: funds return to the original owner.

These functions have different execution paths based on the `htlc_type`,
`HtlcType::Source` or `HtlcType::Destination`.

`pallet-htlc` can be used to exchange assets across two Substrate-based chains,
if both of them include an implementation of the pallet.

### Swap intents

The pallet implements two functions to emulate a naive and simple version of
the `limit-order-protocol`, where makers can make their swap intents public:
- `create_swap_intent`
- `cancel_swap_intent`

The swap intents are both stored on-chain and an event is deposited when
a `SwapIntent` is created or cancelled.

A relayer service can listen for the emitted intentions and forward them
to the resolvers.

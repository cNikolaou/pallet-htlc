# FRAME Pallet for HTLC

The current repo contains a [FRAME pallet](https://paritytech.github.io/polkadot-sdk/master/polkadot_sdk_docs/polkadot_sdk/frame_runtime/index.html),
named `pallet-htlc`, which implements functionality to create
[hashed timelock contracts (HTLC), which can be used for asset swaps](https://1inch.io/assets/1inch-fusion-plus.pdf).

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
to the resolvers. Then the resolvers can source HTLCs with `create_src_htlc`.

The resolvers create `create_dst_htlc` based on events (intents) that happened
on other chains.

## Limitations & missing implementations

The current repo contains a proof-of-concept implementation of HTLCs for asset
swaps as a FRAME pallet that case be used by a Substrate-based chain such
as Polkadot and its parachains.

Below is a list of limitations that can be implemented in the future with more
time for testing.

### Pallet improvements

A list of important, and not exhaustive, improvements on the pallet itself:

**Support various asset to swap to/from**: Currently, the implementation focuses
on swapping to/from the native asset of the Substrate-based chain. To allow
for swaps to/from any asset on the chain

**Pallet configuration parameters**: There are various parameters, such as
`withdrawal_after`, `public_withdrawal_after`, `cancellation_after`, etc,
that are configured by the taker. There should
pallet-wide configuration parameters to allow the users of `pallet-htlc`
to configure the minimum values that want to allow for these.

See for example the:

```rust
#[pallet::constant]
type MinSafetyDeposit: Get<BalanceOf<Self>>;
```
s
**Allow only KYC resolver**: Anyone is allowed to call the functions that
the pallet provides without validating whether a resolver has a successfully
passed a KYC. This can be implemented by gating the resolver-specific functions
with an `ensure!()` that tests for ownership of a non-funglible token.

**Makers should also store a refundable deposit**: A maker can currently
create multiple `SwapIntent`s with unrealistic exchange options by
committing only a small value of the token to be swapped. A malicious
maker can create thousands or `SwapIntent`s with the goal to make the
storage more expensive to run for the users of the `pallet-htlc`.

**Keep track of past SwapIntents**: A lot of data about each `SwapIntent`
are currently stored on-chain. We want to store only the `nonces` and the
hashes of the the `SwapIntent`s that have already been submitted (to avoid
repeated submissions of the same `SwapIntent`s and provide deduplication).

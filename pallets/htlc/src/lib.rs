#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[frame_support::pallet(dev_mode)]
pub mod pallet {
	use frame_support::{
		dispatch::{GetDispatchInfo, RawOrigin},
		pallet_prelude::*,
		traits::{
			fungible,
			fungible::{Mutate, MutateHold},
			tokens::{Precision, Preservation},
		},
	};
	use frame_system::pallet_prelude::*;
	use sp_core::{H160, H256};
	use sp_io::hashing::blake2_256;
	use sp_runtime::traits::{BlakeTwo256, Dispatchable, Hash};
	use sp_std::prelude::*;

	pub type BalanceOf<T> = <<T as Config>::NativeBalance as fungible::Inspect<
		<T as frame_system::Config>::AccountId,
	>>::Balance;

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config {
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

		/// Type to access the Balances Pallet.
		type NativeBalance: fungible::Inspect<Self::AccountId>
			+ fungible::Mutate<Self::AccountId>
			+ fungible::hold::Inspect<Self::AccountId, Reason = Self::RuntimeHoldReason>
			+ fungible::hold::Mutate<Self::AccountId, Reason = Self::RuntimeHoldReason>
			+ fungible::freeze::Inspect<Self::AccountId>
			+ fungible::freeze::Mutate<Self::AccountId>;

		type RuntimeCall: Parameter
			+ Dispatchable<RuntimeOrigin = Self::RuntimeOrigin>
			+ GetDispatchInfo;

		/// Reason for which funds are held.
		type RuntimeHoldReason: From<HoldReason>;

		/// Minimum safety deposit that should be kept when a resolver
		/// creates a HTLC.
		#[pallet::constant]
		type MinSafetyDeposit: Get<BalanceOf<Self>>;
	}

	/// Reason options for held funds.
	#[pallet::composite_enum]
	pub enum HoldReason {
		/// The funds for the recipient of the swap.
		#[codec(index = 0)]
		SwapAmount,
		/// The safety deposit. Goes to whoever calls the withdraw.
		#[codec(index = 1)]
		SafetyDeposit,
		/// Amount held from the maker for each swap intent.
		#[codec(index = 2)]
		MakerSwapIntentAmount,
	}

	/// Immutable parameters of the HTLC, similar to 1inch IBaseEscrow.Immutables
	#[derive(Encode, Decode, TypeInfo, Eq, PartialEq, Clone, Debug)]
	pub struct Immutables<AccountId, Balance, BlockNumber> {
		/// Hash of the cross chain order.
		pub order_hash: H256,
		/// Hash of the maker's secret.
		pub hashlock: H256,
		/// The maker of the swap (on source chain).
		pub maker: AccountId,
		/// The resolver who will complete the swap.
		pub taker: AccountId,
		/// Amount of tokens to swap.
		pub amount: Balance,
		/// Safety deposit in native token.
		pub safety_deposit: Balance,
		/// Timelock parameters
		pub timelocks: Timelocks<BlockNumber>,
	}

	/// Timelock configuration, similar to 1inch TimelocksLib. Store the number
	/// of seconds from the time the escrow contract is deployed.
	#[derive(Encode, Decode, TypeInfo, Eq, PartialEq, Clone, Debug)]
	pub struct Timelocks<BlockNumber> {
		/// Block when the HTLC was deployed.
		pub deployed_at: BlockNumber,
		/// Withdrawal becomes available.
		pub withdrawal_after: BlockNumber,
		/// Public withdrawal becomes available.
		pub public_withdrawal_after: BlockNumber,
		/// Cancellation becomes available.
		pub cancellation_after: BlockNumber,
	}

	/// The status of a HTLC guards against malicious actors who aim to
	/// take incorrect actions.
	#[derive(Encode, Decode, TypeInfo, Eq, PartialEq, Clone, Debug)]
	pub enum HtlcStatus {
		Active,
		Completed,
		Cancelled,
	}

	/// Type of the HTLC to differentiate execution paths between EscrowSrc
	/// and EscrowDst HTL contracts.
	#[derive(Encode, Decode, TypeInfo, Eq, PartialEq, Clone, Debug)]
	pub enum HtlcType {
		Source,
		Destination,
	}

	/// The information for each HTLC that needs to be stored on-chain.
	#[derive(Encode, Decode, TypeInfo)]
	pub struct Htlc<AccountId, Balance, BlockNumber> {
		pub immutables: Immutables<AccountId, Balance, BlockNumber>,
		pub status: HtlcStatus,
		pub htlc_type: HtlcType,
	}

	#[pallet::storage]
	pub type Htlcs<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		H256,
		Htlc<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
		OptionQuery,
	>;

	/// Keep track of the swap intent data of a maker. This can/should be
	/// part of another pallet (such as a limit order protocol pallet) or stored
	#[derive(Encode, Decode, TypeInfo, Eq, PartialEq, Clone, Debug)]
	pub struct SwapIntent<AccountId, Balance, BlockNumber> {
		pub hashlock: H256,
		/// Account that intents to swap
		pub maker: AccountId,
		/// Amount they own and want to provide
		pub src_amount: Balance,
		/// Amount they own and want to receive
		pub dst_amount: Balance,
		/// Address on the destination chain
		pub dst_address: H160,
		pub timeout_after_block: BlockNumber,
		pub nonce: u64,
	}

	/// Enum to keep track of the state of each swap intent submitted
	/// by the maker. We should remove intents after they are completed
	/// or cancelled and keep track of the hash/nonce of the ones that
	/// have already been part of the chain. This is an improvement
	/// over the current implementation that should be implemented.
	#[derive(Encode, Decode, TypeInfo, Eq, PartialEq, Clone, Debug)]
	pub enum IntentStatus<AccountId> {
		/// Intent is active and available for resolvers
		Active,
		/// Intent is being fulfilled (resolver created source HTLC)
		InProgress { resolver: AccountId, htlc_id: H256 },
		/// Intent has been completed successfully
		Completed,
		/// Intent was cancelled by maker
		Cancelled,
		/// Intent expired without fulfillment
		Expired,
	}

	#[derive(Encode, Decode, TypeInfo, Eq, PartialEq, Clone, Debug)]
	pub struct StoredSwapIntent<AccountId, Balance, BlockNumber> {
		pub intent: SwapIntent<AccountId, Balance, BlockNumber>,
		pub status: IntentStatus<AccountId>,
		pub created_at: BlockNumber,
	}

	#[pallet::storage]
	pub type SwapIntents<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		H256,
		StoredSwapIntent<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
		OptionQuery,
	>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// HTLC created.
		HtlcCreated {
			htlc_id: H256,
			hashlock: H256,
			maker: T::AccountId,
			taker: T::AccountId,
			amount: BalanceOf<T>,
			safety_deposit: BalanceOf<T>,
		},
		/// HTLC withdrawn.
		HtlcWithdrawn {
			htlc_id: H256,
			secret: Vec<u8>,
			amount: BalanceOf<T>,
			beneficiary: T::AccountId,
			safety_deposit_recipient: T::AccountId,
		},
		/// HTLC cancelled.
		HtlcCancelled { htlc_id: H256, refund_recipient: T::AccountId },

		/// Swap intent created by maker.
		SwapIntentCreated {
			maker: T::AccountId,
			nonce: u64,
			src_amount: BalanceOf<T>,
			dst_amount: BalanceOf<T>,
			dst_address: H160,
			hashlock: H256,
		},

		/// Swap intent.
		SwapIntentCancelled {
			maker: T::AccountId,
			nonce: u64,
			src_amount: BalanceOf<T>,
			dst_amount: BalanceOf<T>,
			dst_address: H160,
			hashlock: H256,
		},
	}

	#[pallet::error]
	pub enum Error<T> {
		/// Error name example. Should be descriptive.
		NoneValue,

		/// Invalid caller for the operation.
		InvalidCaller,

		/// Invalid timelock configuration.
		InvalidTimelocks,

		/// The provided immutables do not match the ones stored.
		InvalidImmutables,

		/// The hash of the provided secret does not match the hashlock of the contract.
		InvalidSecret,

		/// Cannot lock funds as the caller has insufficient balance.
		InsufficientBalance,

		/// The withdrawal was attempted too early and it's not allowed
		/// based on the current timelock configuration.
		EarlyWithdrawal,

		/// The public withdrawal was attempted too early and it's not allowed
		/// based on the current timelock configuration.
		EarlyPublicWithdrawal,

		/// The cancellation was attempted too early and it's not allowed
		/// based on the current timelock configuration.
		EarlyCancellation,

		/// The withdrawal was attempted too late and it's not allowed
		/// based on the current timelock configuration.
		LateWithdrawal,

		/// The public withdrawal was attempted too late and it's not allowed
		/// based on the current timelock configuration.
		LatePublicWithdrawal,

		/// HTLC already exists.
		HtlcAlreadyExists,

		/// HTLC does not exists.
		HtlcDoesNotExist,

		/// HTLC is not active.
		HtlcNotActive,

		/// Intent already exists.
		IntentAlreadyExists,

		/// Intent does not already exists.
		IntentDoesNotExists,

		/// Intent is not active.
		IntentNotActive,

		/// Intent expired.
		IntentExpired,

		/// A higher value of a safety deposit is required.
		HigherSafetyDepositRequired,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		///////
		/// Calls for destination HTLCs

		#[pallet::call_index(0)]
		pub fn create_dst_htlc(
			origin: OriginFor<T>,
			immutables: Immutables<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
			src_cancellation_timestamp: BlockNumberFor<T>,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;

			// ensure the taker creates the escrow
			ensure!(who == immutables.taker, Error::<T>::InvalidCaller);

			let min_safety_deposit: BalanceOf<T> = T::MinSafetyDeposit::get().into();

			ensure!(
				immutables.safety_deposit >= min_safety_deposit,
				Error::<T>::HigherSafetyDepositRequired
			);

			let current_block = frame_system::Pallet::<T>::block_number();
			let mut updated_immutables = immutables.clone();
			updated_immutables.timelocks.deployed_at = current_block;

			// ensure cancellation time aligns with source chain cancellation
			ensure!(
				updated_immutables.timelocks.cancellation_after <= src_cancellation_timestamp,
				Error::<T>::InvalidTimelocks
			);

			// validate timelock sequence (withdrawal < public_withdrawal < cancellation)
			ensure!(
				updated_immutables.timelocks.withdrawal_after <=
					updated_immutables.timelocks.public_withdrawal_after &&
					updated_immutables.timelocks.public_withdrawal_after <=
						updated_immutables.timelocks.cancellation_after,
				Error::<T>::InvalidTimelocks
			);

			// ensure HTLC doesn't already exist
			let htlc_id = Self::hash_immutables(&immutables);
			ensure!(!Htlcs::<T>::contains_key(&htlc_id), Error::<T>::HtlcAlreadyExists);

			// hold the required funds for the swap and then the safety deposit
			T::NativeBalance::hold(&HoldReason::SwapAmount.into(), &who, updated_immutables.amount)
				.map_err(|_| Error::<T>::InsufficientBalance)?;

			T::NativeBalance::hold(
				&HoldReason::SafetyDeposit.into(),
				&who,
				updated_immutables.safety_deposit,
			)
			.map_err(|_| Error::<T>::InsufficientBalance)?;

			let htlc = Htlc {
				immutables: immutables.clone(),
				status: HtlcStatus::Active,
				htlc_type: HtlcType::Destination,
			};

			Htlcs::<T>::insert(&htlc_id, &htlc);

			Self::deposit_event(Event::HtlcCreated {
				htlc_id,
				hashlock: immutables.hashlock,
				maker: immutables.maker,
				taker: immutables.taker,
				amount: immutables.amount,
				safety_deposit: updated_immutables.safety_deposit,
			});

			Ok(())
		}

		#[pallet::call_index(1)]
		pub fn withdraw(
			origin: OriginFor<T>,
			immutables: Immutables<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
			secret: Vec<u8>,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;

			// Validation phase

			// validate HTLC exists
			let htlc_id = Self::hash_immutables(&immutables);
			let mut htlc = Htlcs::<T>::get(&htlc_id).ok_or(Error::<T>::HtlcDoesNotExist)?;
			ensure!(htlc.status == HtlcStatus::Active, Error::<T>::HtlcNotActive);

			// verify immutables match
			ensure!(htlc.immutables == immutables, Error::<T>::InvalidImmutables);

			// verify secret hash matches the one stored in the lock
			let secret_hash = BlakeTwo256::hash(&secret);
			ensure!(htlc.immutables.hashlock == secret_hash, Error::<T>::InvalidSecret);

			// verify taker is the caller of the external
			ensure!(who == htlc.immutables.taker, Error::<T>::InvalidCaller);

			// check the timing is valid for the withdrawal
			let current_block = frame_system::Pallet::<T>::block_number();
			ensure!(
				current_block >= htlc.immutables.timelocks.withdrawal_after,
				Error::<T>::EarlyWithdrawal
			);
			ensure!(
				current_block < htlc.immutables.timelocks.cancellation_after,
				Error::<T>::LateWithdrawal
			);

			// Withdrawal phase

			let beneficiary;

			match htlc.htlc_type {
				HtlcType::Destination => {
					// Destination HTLC: EVM -> Polkadot
					// Resolver (taker) deposited funds for maker
					// Funds go: taker -> maker
					T::NativeBalance::release(
						&HoldReason::SwapAmount.into(),
						&htlc.immutables.taker,
						htlc.immutables.amount,
						Precision::Exact,
					)?;

					T::NativeBalance::transfer(
						&htlc.immutables.taker,
						&htlc.immutables.maker,
						htlc.immutables.amount,
						Preservation::Preserve,
					)?;

					beneficiary = htlc.immutables.maker.clone();
				},

				HtlcType::Source => {
					// Destination HTLC: Polkadot -> EVM
					// Maker deposited funds for taker
					// Funds go: maker -> taker
					T::NativeBalance::release(
						&HoldReason::MakerSwapIntentAmount.into(),
						&htlc.immutables.maker,
						htlc.immutables.amount,
						Precision::Exact,
					)?;

					T::NativeBalance::transfer(
						&htlc.immutables.maker,
						&htlc.immutables.taker,
						htlc.immutables.amount,
						Preservation::Preserve,
					)?;

					beneficiary = htlc.immutables.taker.clone();
				},
			}

			// Safety deposit back to taker
			T::NativeBalance::release(
				&HoldReason::SafetyDeposit.into(),
				&htlc.immutables.taker,
				htlc.immutables.safety_deposit,
				Precision::Exact,
			)?;

			// update HTLC
			htlc.status = HtlcStatus::Completed;
			Htlcs::<T>::insert(&htlc_id, &htlc);

			// emit event that shows the unhashed secret to the public
			Self::deposit_event(Event::HtlcWithdrawn {
				htlc_id,
				secret,
				amount: immutables.amount,
				beneficiary,
				safety_deposit_recipient: who,
			});

			Ok(())
		}

		#[pallet::call_index(2)]
		pub fn public_withdraw(
			origin: OriginFor<T>,
			immutables: Immutables<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
			secret: Vec<u8>,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;

			// Validation phase

			// validate HTLC exists
			let htlc_id = Self::hash_immutables(&immutables);
			let mut htlc = Htlcs::<T>::get(&htlc_id).ok_or(Error::<T>::HtlcDoesNotExist)?;
			ensure!(htlc.status == HtlcStatus::Active, Error::<T>::HtlcNotActive);

			// verify immutables match
			ensure!(htlc.immutables == immutables, Error::<T>::InvalidImmutables);

			// verify secret hash matches the one stored in the lock
			let secret_hash = BlakeTwo256::hash(&secret);
			ensure!(htlc.immutables.hashlock == secret_hash, Error::<T>::InvalidSecret);

			// Verify taker is not the caller of the external; anyone else
			// can call this function. The check here is not as important as
			// the check of the complementary condition in the `withdraw`
			// function.
			ensure!(who != htlc.immutables.taker, Error::<T>::InvalidCaller);

			// check the timing is valid for the public withdrawal
			let current_block = frame_system::Pallet::<T>::block_number();
			ensure!(
				current_block >= htlc.immutables.timelocks.public_withdrawal_after,
				Error::<T>::EarlyPublicWithdrawal
			);
			ensure!(
				current_block < htlc.immutables.timelocks.cancellation_after,
				Error::<T>::LatePublicWithdrawal
			);

			// Withdrawal phase

			let beneficiary;

			match htlc.htlc_type {
				HtlcType::Destination => {
					// Destination HTLC: EVM -> Polkadot
					// Resolver (taker) deposited funds for maker
					// Funds go: taker -> maker
					T::NativeBalance::release(
						&HoldReason::SwapAmount.into(),
						&htlc.immutables.taker,
						htlc.immutables.amount,
						Precision::Exact,
					)?;

					T::NativeBalance::transfer(
						&htlc.immutables.taker,
						&htlc.immutables.maker,
						htlc.immutables.amount,
						Preservation::Preserve,
					)?;

					beneficiary = htlc.immutables.maker.clone();
				},

				HtlcType::Source => {
					// Destination HTLC: Polkadot -> EVM
					// Maker deposited funds for taker
					// Funds go: maker -> taker
					T::NativeBalance::release(
						&HoldReason::MakerSwapIntentAmount.into(),
						&htlc.immutables.maker,
						htlc.immutables.amount,
						Precision::Exact,
					)?;

					T::NativeBalance::transfer(
						&htlc.immutables.maker,
						&htlc.immutables.taker,
						htlc.immutables.amount,
						Preservation::Preserve,
					)?;

					beneficiary = htlc.immutables.taker.clone();
				},
			}

			// release safety deposit to the take
			T::NativeBalance::release(
				&HoldReason::SafetyDeposit.into(),
				&htlc.immutables.taker,
				htlc.immutables.safety_deposit,
				Precision::Exact,
			)?;

			T::NativeBalance::transfer(
				&htlc.immutables.taker,
				&who,
				htlc.immutables.safety_deposit,
				Preservation::Preserve,
			)?;

			// update HTLC
			htlc.status = HtlcStatus::Completed;
			Htlcs::<T>::insert(&htlc_id, &htlc);

			// emit event that shows the unhashed secret to the public
			Self::deposit_event(Event::HtlcWithdrawn {
				htlc_id,
				secret,
				amount: immutables.amount,
				beneficiary,
				safety_deposit_recipient: who,
			});

			Ok(())
		}

		#[pallet::call_index(3)]
		pub fn cancel(
			origin: OriginFor<T>,
			immutables: Immutables<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;

			// Validation phase

			// validate HTLC exists
			let htlc_id = Self::hash_immutables(&immutables);
			let mut htlc = Htlcs::<T>::get(&htlc_id).ok_or(Error::<T>::HtlcDoesNotExist)?;
			ensure!(htlc.status == HtlcStatus::Active, Error::<T>::HtlcNotActive);

			// verify immutables match
			ensure!(htlc.immutables == immutables, Error::<T>::InvalidImmutables);

			// Verify taker is not the caller of the external; anyone else
			// can call this function. The check here is not as important as
			// the check of the complementary condition in the `withdraw`
			// function.
			ensure!(who == htlc.immutables.taker, Error::<T>::InvalidCaller);

			// check the timing is valid for the public withdrawal
			let current_block = frame_system::Pallet::<T>::block_number();
			ensure!(
				current_block >= htlc.immutables.timelocks.cancellation_after,
				Error::<T>::EarlyCancellation
			);

			// Canellation phase
			let refund_recipient;

			match htlc.htlc_type {
				HtlcType::Destination => {
					// Destination HTLC: EVM -> Polkadot
					// Resolver (taker) deposited funds for maker
					// Funds go back to taker
					T::NativeBalance::release(
						&HoldReason::SwapAmount.into(),
						&htlc.immutables.taker,
						htlc.immutables.amount,
						Precision::Exact,
					)?;

					// release safety deposit to the take
					T::NativeBalance::release(
						&HoldReason::SafetyDeposit.into(),
						&htlc.immutables.taker,
						htlc.immutables.safety_deposit,
						Precision::Exact,
					)?;

					refund_recipient = htlc.immutables.taker.clone();
				},

				HtlcType::Source => {
					// Destination HTLC: Polkadot -> EVM
					// Maker deposited funds for taker
					// Funds go back to maker
					T::NativeBalance::release(
						&HoldReason::MakerSwapIntentAmount.into(),
						&htlc.immutables.maker,
						htlc.immutables.amount,
						Precision::Exact,
					)?;

					// release safety deposit to the take
					T::NativeBalance::release(
						&HoldReason::SafetyDeposit.into(),
						&htlc.immutables.taker,
						htlc.immutables.safety_deposit,
						Precision::Exact,
					)?;

					refund_recipient = htlc.immutables.maker.clone();
				},
			}

			// update HTLC
			htlc.status = HtlcStatus::Cancelled;
			Htlcs::<T>::insert(&htlc_id, &htlc);

			// emit event that shows the unhashed secret to the public
			Self::deposit_event(Event::HtlcCancelled { htlc_id, refund_recipient });

			Ok(())
		}

		///////
		/// Calls for Swap intents

		#[pallet::call_index(4)]
		pub fn create_swap_intent(
			origin: OriginFor<T>,
			intent: SwapIntent<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;

			// ensure the maker creates the intent to swap
			ensure!(who == intent.maker, Error::<T>::InvalidCaller);

			// generate the key for the map and check it doesn't already exist
			let intent_key = Self::intent_key(&who, intent.nonce);
			ensure!(!SwapIntents::<T>::contains_key(&intent_key), Error::<T>::IntentAlreadyExists);

			let current_block = frame_system::Pallet::<T>::block_number();
			let stored_intent = StoredSwapIntent {
				intent: intent.clone(),
				status: IntentStatus::Active,
				created_at: current_block,
			};

			SwapIntents::<T>::insert(&intent_key, &stored_intent);

			T::NativeBalance::hold(
				&HoldReason::MakerSwapIntentAmount.into(),
				&who,
				intent.src_amount,
			)
			.map_err(|_| Error::<T>::InsufficientBalance)?;

			Self::deposit_event(Event::SwapIntentCreated {
				maker: who,
				nonce: intent.nonce,
				src_amount: intent.src_amount,
				dst_amount: intent.dst_amount,
				dst_address: intent.dst_address,
				hashlock: intent.hashlock,
			});

			Ok(())
		}

		#[pallet::call_index(5)]
		pub fn cancel_swap_intent(origin: OriginFor<T>, nonce: u64) -> DispatchResult {
			let who = ensure_signed(origin)?;

			// generate the key for the map and check it doesn't already exist
			let intent_key = Self::intent_key(&who, nonce);
			let mut stored_intent =
				SwapIntents::<T>::get(&intent_key).ok_or(Error::<T>::IntentDoesNotExists)?;

			// ensure we cannot cancel an already cancelled intent
			ensure!(stored_intent.status == IntentStatus::Active, Error::<T>::IntentNotActive);

			// ensure the maker cancels the intent to swap
			ensure!(who == stored_intent.intent.maker, Error::<T>::InvalidCaller);

			stored_intent.status = IntentStatus::Cancelled;
			SwapIntents::<T>::insert(&intent_key, &stored_intent);

			T::NativeBalance::release(
				&HoldReason::MakerSwapIntentAmount.into(),
				&who,
				stored_intent.intent.src_amount,
				Precision::Exact,
			)?;

			Self::deposit_event(Event::SwapIntentCancelled {
				maker: who,
				nonce,
				src_amount: stored_intent.intent.src_amount,
				dst_amount: stored_intent.intent.dst_amount,
				dst_address: stored_intent.intent.dst_address,
				hashlock: stored_intent.intent.hashlock,
			});

			Ok(())
		}

		///////
		/// Calls for source HTLCs

		#[pallet::call_index(6)]
		pub fn create_src_htlc(
			origin: OriginFor<T>,
			maker: T::AccountId,
			nonce: u64,
			timelocks: Timelocks<BlockNumberFor<T>>,
			safety_deposit: BalanceOf<T>,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;

			let min_safety_deposit: BalanceOf<T> = T::MinSafetyDeposit::get().into();

			ensure!(safety_deposit >= min_safety_deposit, Error::<T>::HigherSafetyDepositRequired);

			// generate the key for the map and check it doesn't already exist
			let intent_key = Self::intent_key(&maker, nonce);
			let stored_intent =
				SwapIntents::<T>::get(&intent_key).ok_or(Error::<T>::IntentDoesNotExists)?;

			// ensure we cannot cancel an already cancelled intent
			ensure!(stored_intent.status == IntentStatus::Active, Error::<T>::IntentNotActive);

			// ensure the intent hasn't expired
			let current_block = frame_system::Pallet::<T>::block_number();
			ensure!(
				current_block <= stored_intent.intent.timeout_after_block,
				Error::<T>::IntentExpired
			);

			// validate timelock sequence (withdrawal < public_withdrawal < cancellation)
			ensure!(
				timelocks.deployed_at <= timelocks.cancellation_after &&
					timelocks.withdrawal_after <= timelocks.public_withdrawal_after &&
					timelocks.public_withdrawal_after <= timelocks.cancellation_after,
				Error::<T>::InvalidTimelocks
			);

			let immutables = Immutables {
				order_hash: intent_key,
				hashlock: stored_intent.intent.hashlock,
				maker: stored_intent.intent.maker.clone(),
				taker: who.clone(),
				amount: stored_intent.intent.src_amount,
				safety_deposit,
				timelocks,
			};

			// ensure HTLC doesn't already exist
			let htlc_id = Self::hash_immutables(&immutables);
			ensure!(!Htlcs::<T>::contains_key(&htlc_id), Error::<T>::HtlcAlreadyExists);

			// hold the required safety deposit for the swap from the taker
			T::NativeBalance::hold(
				&HoldReason::SafetyDeposit.into(),
				&who,
				immutables.safety_deposit,
			)
			.map_err(|_| Error::<T>::InsufficientBalance)?;

			let htlc = Htlc {
				immutables: immutables.clone(),
				status: HtlcStatus::Active,
				htlc_type: HtlcType::Source,
			};

			Htlcs::<T>::insert(&htlc_id, &htlc);

			Self::deposit_event(Event::HtlcCreated {
				htlc_id,
				hashlock: stored_intent.intent.hashlock,
				maker: stored_intent.intent.maker,
				taker: who,
				amount: stored_intent.intent.src_amount,
				safety_deposit,
			});

			Ok(())
		}
	}

	impl<T: Config> Pallet<T> {
		/// Generate unique ID from immutables
		pub fn hash_immutables(
			immutables: &Immutables<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
		) -> H256 {
			let encoded = immutables.encode();
			BlakeTwo256::hash(&encoded)
		}

		/// Geenrate intent storage key from maker AccountId + nonce
		pub fn intent_key(maker: &T::AccountId, nonce: u64) -> H256 {
			let mut data = maker.encode();
			data.extend_from_slice(&nonce.to_le_bytes());
			BlakeTwo256::hash(&data)
		}
	}
}

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
	use sp_core::H256;
	use sp_io::hashing::blake2_256;
	use sp_runtime::traits::{BlakeTwo256, Dispatchable, Hash, TrailingZeroInput};
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

		type RuntimeHoldReason: From<HoldReason>;
	}

	#[pallet::composite_enum]
	pub enum HoldReason {
		/// The funds for the recipient of the swap.
		#[codec(index = 0)]
		SwapAmount,
		/// The safety deposit. Goes to whoever calls the withdraw.
		#[codec(index = 1)]
		SafetyDeposit,
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

	#[derive(Encode, Decode, TypeInfo, Eq, PartialEq, Clone, Debug)]
	pub enum HtlcStatus {
		Active,
		Completed,
		Cancelled,
	}

	#[derive(Encode, Decode, TypeInfo)]
	pub struct Htlc<AccountId, Balance, BlockNumber> {
		pub immutables: Immutables<AccountId, Balance, BlockNumber>,
		pub status: HtlcStatus,
	}

	#[pallet::storage]
	pub type Htlcs<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		H256,
		Htlc<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
		OptionQuery,
	>;

	#[pallet::storage]
	pub type ReservedDeposits<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		(T::AccountId, H256),
		(BalanceOf<T>, BalanceOf<T>),
		OptionQuery,
	>;

	#[pallet::storage]
	pub type Something<T> = StorageValue<Value = u32>;
	#[pallet::storage]
	pub type SomethingMap<T: Config> = StorageMap<Key = T::AccountId, Value = BlockNumberFor<T>>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// Event example.
		SomethingStored { something: u32, who: T::AccountId },
		/// HTLC created.
		HtlcCreated {
			htlc_id: H256,
			hashlock: H256,
			maker: T::AccountId,
			taker: T::AccountId,
			amount: BalanceOf<T>,
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
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::call_index(0)]
		pub fn create_dst_htlc(
			origin: OriginFor<T>,
			immutables: Immutables<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
			src_cancellation_timestamp: BlockNumberFor<T>,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;

			// ensure the taker creates the escrow
			ensure!(who == immutables.taker, Error::<T>::InvalidCaller);

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

			let htlc = Htlc { immutables: updated_immutables.clone(), status: HtlcStatus::Active };

			Htlcs::<T>::insert(&htlc_id, &htlc);

			ReservedDeposits::<T>::insert(
				(&who, &htlc_id),
				(updated_immutables.amount, updated_immutables.safety_deposit),
			);

			Self::deposit_event(Event::HtlcCreated {
				htlc_id,
				hashlock: immutables.hashlock,
				maker: immutables.maker,
				taker: immutables.taker,
				amount: immutables.amount,
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

			// release & transfer swap amount to maker
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

			// release safety deposit to the take
			T::NativeBalance::release(
				&HoldReason::SafetyDeposit.into(),
				&htlc.immutables.taker,
				htlc.immutables.safety_deposit,
				Precision::Exact,
			)?;

			// update HTLC
			htlc.status = HtlcStatus::Completed;
			Htlcs::<T>::insert(&htlc_id, &htlc);

			ReservedDeposits::<T>::remove((&htlc.immutables.taker, &htlc_id));

			// emit event that shows the unhashed secret to the public
			Self::deposit_event(Event::HtlcWithdrawn {
				htlc_id,
				secret,
				amount: immutables.amount,
				beneficiary: htlc.immutables.maker,
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

			// release & transfer swap amount to maker
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

			ReservedDeposits::<T>::remove((&htlc.immutables.taker, &htlc_id));

			// emit event that shows the unhashed secret to the public
			Self::deposit_event(Event::HtlcWithdrawn {
				htlc_id,
				secret,
				amount: immutables.amount,
				beneficiary: htlc.immutables.maker,
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

			// Withdrawal phase

			// release & transfer swap amount to maker
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

			// update HTLC
			htlc.status = HtlcStatus::Cancelled;
			Htlcs::<T>::insert(&htlc_id, &htlc);

			ReservedDeposits::<T>::remove((&htlc.immutables.taker, &htlc_id));

			// emit event that shows the unhashed secret to the public
			Self::deposit_event(Event::HtlcCancelled { htlc_id, refund_recipient: who });

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
	}
}

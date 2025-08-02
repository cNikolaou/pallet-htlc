use crate::{mock::*, *};
use frame_support::{
	assert_noop, assert_ok,
	traits::{fungible::InspectHold, Get},
};
use sp_core::{blake2_256, H256};

const ALICE: u64 = 1;
const RESOLVER_BOB: u64 = 2;
const RESOLVER_CHARLIE: u64 = 3;

fn create_test_htlc_immutables(
	maker: u64,
	taker: u64,
	amount: u128,
	safety_deposit: u128,
	current_block: u64,
) -> Immutables<u64, u128, u64> {
	let secret = b"tests_secret";
	let hashlock = H256(blake2_256(secret));

	Immutables {
		order_hash: H256(blake2_256(b"Order hash")),
		hashlock,
		maker,
		taker,
		amount,
		safety_deposit,
		timelocks: Timelocks {
			deployed_at: 0,
			withdrawal_after: current_block + 100,
			public_withdrawal_after: current_block + 200,
			cancellation_after: current_block + 300,
		},
	}
}

#[test]
fn it_works_for_default_value() {
	new_test_ext().execute_with(|| {
		// Go past genesis block so events get deposited
		System::set_block_number(1);
		// Dispatch a signed extrinsic.
		assert_ok!(HtlcEscrow::store_something(RuntimeOrigin::signed(1), 42));
		// Read pallet storage and assert an expected result.
		assert_eq!(Something::<Test>::get(), Some(42));
		// Assert that the correct event was deposited
		System::assert_last_event(Event::SomethingStored { something: 42, who: 1 }.into());
	});
}

#[test]
fn correct_error_for_none_value() {
	new_test_ext().execute_with(|| {
		// Ensure the expected error is thrown when no value is present.
		assert_noop!(
			HtlcEscrow::retrieve_something(RuntimeOrigin::signed(1)),
			Error::<Test>::NoneValue
		);
	});
}

#[test]
fn create_htlc_and_reserve_funds() {
	new_test_ext().execute_with(|| {
		// track events
		System::set_block_number(1);

		// initial setup
		let maker = ALICE;
		let taker = RESOLVER_BOB;
		let swap_amount = 1000u128;
		let safety_deposit = 100u128;
		let current_block = 1u64;
		let src_cancellation_timestamp = current_block + 400u64;

		// create test immutables
		let immutables =
			create_test_htlc_immutables(maker, taker, swap_amount, safety_deposit, current_block);

		// verify initial balances
		assert_eq!(Balances::free_balance(&taker), 1000000);
		assert_eq!(Balances::total_balance_on_hold(&taker), 0u128);

		// create HTLC
		assert_ok!(HtlcEscrow::create_dst_htlc(
			RuntimeOrigin::signed(taker),
			immutables.clone(),
			src_cancellation_timestamp,
		));

		// verify reserved funds
		let total_reserved = swap_amount + safety_deposit;
		assert_eq!(Balances::free_balance(&taker), 1000000 - total_reserved);
		assert_eq!(
			Balances::balance_on_hold(&crate::HoldReason::SwapAmount.into(), &taker),
			swap_amount
		);
		assert_eq!(
			Balances::balance_on_hold(&crate::HoldReason::SafetyDeposit.into(), &taker),
			safety_deposit
		);

		// verify HTLC is stored correclty
		let htlc_id = HtlcEscrow::hash_immutables(&immutables);
		let stored_htlc = Htlcs::<Test>::get(&htlc_id).expect("HTLC id is contained; qed");

		assert_eq!(stored_htlc.status, HtlcStatus::Active);
		assert_eq!(stored_htlc.immutables.timelocks.deployed_at, current_block);
		assert_eq!(stored_htlc.immutables.amount, swap_amount);
		assert_eq!(stored_htlc.immutables.safety_deposit, safety_deposit);
		assert_eq!(stored_htlc.immutables.maker, maker);
		assert_eq!(stored_htlc.immutables.taker, taker);

		// verify deposited event
		System::assert_last_event(
			Event::HtlcCreated {
				htlc_id,
				hashlock: immutables.hashlock,
				maker,
				taker,
				amount: swap_amount,
			}
			.into(),
		);
	});
}

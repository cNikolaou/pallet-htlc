use crate::{mock::*, *};
use frame_support::{
	assert_noop, assert_ok,
	traits::{fungible::InspectHold, Get},
};
use sp_core::{blake2_256, H256};

const ALICE: u64 = 1;
const RESOLVER_BOB: u64 = 2;
const RESOLVER_CHARLIE: u64 = 3;

const SWAP_AMOUNT: u128 = 1000;
const SAFETY_DEPOSIT: u128 = 100;

fn hash_secret(secret: &[u8]) -> H256 {
	H256(blake2_256(secret))
}

fn create_test_htlc_immutables(
	hashlock: H256,
	maker: u64,
	taker: u64,
	amount: u128,
	safety_deposit: u128,
	current_block: u64,
) -> Immutables<u64, u128, u64> {
	Immutables {
		order_hash: H256(blake2_256(b"Order hash")),
		hashlock,
		maker,
		taker,
		amount,
		safety_deposit,
		timelocks: Timelocks {
			deployed_at: current_block,
			withdrawal_after: current_block + 100,
			public_withdrawal_after: current_block + 200,
			cancellation_after: current_block + 300,
		},
	}
}

#[test]
fn create_htlc_and_reserve_funds() {
	new_test_ext().execute_with(|| {
		// track events
		System::set_block_number(1);

		// initial setup
		let maker = ALICE;
		let taker = RESOLVER_BOB;

		let swap_amount = SWAP_AMOUNT;
		let safety_deposit = SAFETY_DEPOSIT;

		let secret = b"tests_secret";
		let hashlock = hash_secret(secret);

		let current_block = 1u64;
		let src_cancellation_timestamp = current_block + 400u64;

		// verify initial balances
		assert_eq!(Balances::free_balance(&taker), 1000000);
		assert_eq!(Balances::total_balance_on_hold(&taker), 0u128);

		// create immutables for the test
		let immutables = create_test_htlc_immutables(
			hashlock,
			maker,
			taker,
			swap_amount,
			safety_deposit,
			current_block,
		);

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
		assert_eq!(stored_htlc.immutables.amount, immutables.amount);
		assert_eq!(stored_htlc.immutables.safety_deposit, immutables.safety_deposit);
		assert_eq!(stored_htlc.immutables.maker, immutables.maker);
		assert_eq!(stored_htlc.immutables.taker, immutables.taker);
		assert_eq!(stored_htlc.immutables.timelocks, immutables.timelocks);

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

#[test]
fn withdraw_success_with_valid_secret() {
	new_test_ext().execute_with(|| {
		// track events
		System::set_block_number(1);

		// initial setup
		let maker = ALICE;
		let taker = RESOLVER_BOB;

		let swap_amount = SWAP_AMOUNT;
		let safety_deposit = SAFETY_DEPOSIT;

		let current_block = 1u64;
		let src_cancellation_timestamp = current_block + 400u64;

		let secret = b"tests_secret";
		let hashlock = hash_secret(secret);

		// verify initial balances
		assert_eq!(Balances::free_balance(&taker), 1000000);
		assert_eq!(Balances::free_balance(&maker), 1000000);
		assert_eq!(Balances::total_balance_on_hold(&taker), 0u128);

		// create immutables for the test
		let immutables = create_test_htlc_immutables(
			hashlock,
			maker,
			taker,
			swap_amount,
			safety_deposit,
			current_block,
		);

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

		// maker should still have the same balance as before; no withdrawal yet
		assert_eq!(Balances::free_balance(&maker), 1000000);

		// verify HTLC is stored correclty
		let htlc_id = HtlcEscrow::hash_immutables(&immutables);
		let stored_htlc = Htlcs::<Test>::get(&htlc_id).expect("HTLC id is contained; qed");

		assert_eq!(stored_htlc.status, HtlcStatus::Active);
		assert_eq!(stored_htlc.immutables.timelocks.deployed_at, current_block);
		assert_eq!(stored_htlc.immutables.amount, immutables.amount);
		assert_eq!(stored_htlc.immutables.safety_deposit, immutables.safety_deposit);
		assert_eq!(stored_htlc.immutables.maker, immutables.maker);
		assert_eq!(stored_htlc.immutables.taker, immutables.taker);

		// attempt early withdrawal by `taker` should fail
		assert_noop!(
			HtlcEscrow::withdraw(RuntimeOrigin::signed(taker), immutables.clone(), secret.to_vec(),),
			Error::<Test>::EarlyWithdrawal,
		);

		// Move to withdrawal period + 10 extra blocks
		let after_withdrawal_block = immutables.timelocks.withdrawal_after + 10;
		System::set_block_number(after_withdrawal_block);

		assert_ok!(HtlcEscrow::withdraw(
			RuntimeOrigin::signed(taker),
			immutables.clone(),
			secret.to_vec(),
		));

		// verify the balance has moved to the maker
		assert_eq!(Balances::free_balance(&maker), 1000000 + swap_amount);
		assert_eq!(Balances::free_balance(&taker), 1000000 - swap_amount);
		assert_eq!(Balances::balance_on_hold(&crate::HoldReason::SafetyDeposit.into(), &taker), 0);

		let stored_htlc = Htlcs::<Test>::get(&htlc_id).expect("HTLC id is contained; qed");
		assert_eq!(stored_htlc.status, HtlcStatus::Completed);

		// verify deposited event
		System::assert_last_event(
			Event::HtlcWithdrawn {
				htlc_id,
				secret: secret.to_vec(),
				amount: swap_amount,
				beneficiary: maker,
				safety_deposit_recipient: taker,
			}
			.into(),
		);
	});
}

#[test]
fn public_withdraw_success_by_third_party() {
	new_test_ext().execute_with(|| {
		// track events
		System::set_block_number(1);

		// initial setup
		let maker = ALICE;
		let taker = RESOLVER_BOB;
		let third_party = RESOLVER_CHARLIE;

		let swap_amount = SWAP_AMOUNT;
		let safety_deposit = SAFETY_DEPOSIT;

		let current_block = 1u64;
		let src_cancellation_timestamp = current_block + 400u64;

		let secret = b"tests_secret";
		let hashlock = hash_secret(secret);

		// verify initial balances
		assert_eq!(Balances::free_balance(&maker), 1000000);
		assert_eq!(Balances::free_balance(&taker), 1000000);
		assert_eq!(Balances::total_balance_on_hold(&taker), 0u128);
		assert_eq!(Balances::free_balance(&third_party), 1000000);

		// create immutables for the test
		let immutables = create_test_htlc_immutables(
			hashlock,
			maker,
			taker,
			swap_amount,
			safety_deposit,
			current_block,
		);

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

		// maker should still have the same balance as before; no withdrawal yet
		assert_eq!(Balances::free_balance(&maker), 1000000);

		// verify HTLC is stored correclty
		let htlc_id = HtlcEscrow::hash_immutables(&immutables);
		let stored_htlc = Htlcs::<Test>::get(&htlc_id).expect("HTLC id is contained; qed");

		assert_eq!(stored_htlc.status, HtlcStatus::Active);
		assert_eq!(stored_htlc.immutables.timelocks.deployed_at, current_block);
		assert_eq!(stored_htlc.immutables.amount, immutables.amount);
		assert_eq!(stored_htlc.immutables.safety_deposit, immutables.safety_deposit);
		assert_eq!(stored_htlc.immutables.maker, immutables.maker);
		assert_eq!(stored_htlc.immutables.taker, immutables.taker);

		// attempt early public withdrawal by `third_party` should fail
		assert_noop!(
			HtlcEscrow::public_withdraw(
				RuntimeOrigin::signed(third_party),
				immutables.clone(),
				secret.to_vec(),
			),
			Error::<Test>::EarlyPublicWithdrawal,
		);

		// Move to withdrawal period + 10 extra blocks
		let withdrawal_block = immutables.timelocks.withdrawal_after + 10;
		System::set_block_number(withdrawal_block);

		// attempt early withdrawal by `third_party` should still fail;
		// the `third_party` hasn't waited enough
		assert_noop!(
			HtlcEscrow::public_withdraw(
				RuntimeOrigin::signed(third_party),
				immutables.clone(),
				secret.to_vec(),
			),
			Error::<Test>::EarlyPublicWithdrawal,
		);

		// `third_party` calls the `withdraw` and fails; only the taker
		// can call that
		assert_noop!(
			HtlcEscrow::withdraw(
				RuntimeOrigin::signed(third_party),
				immutables.clone(),
				secret.to_vec(),
			),
			Error::<Test>::InvalidCaller,
		);

		// Move to public withdrawal period + 10 extra blocks
		let after_public_withdrawal_block = immutables.timelocks.public_withdrawal_after + 10;
		System::set_block_number(after_public_withdrawal_block);

		// third_party calls the `public_withdraw`
		assert_ok!(HtlcEscrow::public_withdraw(
			RuntimeOrigin::signed(third_party),
			immutables.clone(),
			secret.to_vec(),
		));

		// verify the balance has moved to the maker and the safety_deposit
		// to the `third_party` resolver as the `taker` did not call `withdraw`
		assert_eq!(Balances::free_balance(&maker), 1000000 + swap_amount);
		assert_eq!(Balances::free_balance(&taker), 1000000 - swap_amount - safety_deposit);
		assert_eq!(Balances::free_balance(&third_party), 1000000 + safety_deposit);
		assert_eq!(Balances::balance_on_hold(&crate::HoldReason::SafetyDeposit.into(), &taker), 0);

		let stored_htlc = Htlcs::<Test>::get(&htlc_id).expect("HTLC id is contained; qed");
		assert_eq!(stored_htlc.status, HtlcStatus::Completed);

		// verify deposited event that mentions the `third_party` as the
		// recipient for the safety deposit
		System::assert_last_event(
			Event::HtlcWithdrawn {
				htlc_id,
				secret: secret.to_vec(),
				amount: swap_amount,
				beneficiary: maker,
				safety_deposit_recipient: third_party,
			}
			.into(),
		);
	});
}

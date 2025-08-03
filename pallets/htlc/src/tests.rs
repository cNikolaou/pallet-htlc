use crate::{mock::*, *};
use frame_support::{
	assert_noop, assert_ok,
	traits::{fungible::InspectHold, Get},
};
use sp_core::{blake2_256, H160, H256};

const ALICE: u64 = 1;
const RESOLVER_BOB: u64 = 2;
const RESOLVER_CHARLIE: u64 = 3;

const SWAP_AMOUNT: u128 = 1000;
const SAFETY_DEPOSIT: u128 = 100;

const SRC_AMOUNT: u128 = 1000;
const DST_AMOUNT: u128 = 2000;

// const WITHDRAWAL_AFTER_BLOCKS: u64 = 100;
// const PUBLIC_WITHDRAWAL_AFTER_BLOCKS: u64 = 200;
// const WITHDRAWAL_AFTER_BLOCKS: u64 = 300;

fn hash_of_word(word: &[u8]) -> H256 {
	H256(blake2_256(word))
}

fn create_timelocks(current_block: u64) -> Timelocks<u64> {
	Timelocks {
		deployed_at: current_block,
		withdrawal_after: current_block + 100,
		public_withdrawal_after: current_block + 200,
		cancellation_after: current_block + 300,
	}
}

fn create_test_htlc_immutables(
	order_hash: H256,
	hashlock: H256,
	maker: u64,
	taker: u64,
	amount: u128,
	safety_deposit: u128,
	current_block: u64,
) -> Immutables<u64, u128, u64> {
	let timelocks = create_timelocks(current_block);

	Immutables { order_hash, hashlock, maker, taker, amount, safety_deposit, timelocks }
}

fn get_h160_addr(address: u64) -> H160 {
	let mut addr_bytes = [0u8; 20];
	addr_bytes[12..20].copy_from_slice(&address.to_be_bytes());
	H160::from(addr_bytes)
}

fn create_swap_intent(
	hashlock: H256,
	maker: u64,
	src_amount: u128,
	dst_amount: u128,
	dst_address: H160,
	timeout_after_block: u64,
	nonce: u64,
) -> SwapIntent<u64, u128, u64> {
	SwapIntent { hashlock, maker, src_amount, dst_amount, dst_address, timeout_after_block, nonce }
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
		let hashlock = hash_of_word(secret);

		let order_hash = hash_of_word(b"order hash");

		let current_block = 1u64;
		let src_cancellation_timestamp = current_block + 400u64;

		// verify initial balances
		assert_eq!(Balances::free_balance(&taker), 1000000);
		assert_eq!(Balances::total_balance_on_hold(&taker), 0u128);

		// create immutables for the test
		let immutables = create_test_htlc_immutables(
			order_hash,
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
				safety_deposit,
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
		let hashlock = hash_of_word(secret);
		let order_hash = hash_of_word(b"order hash");

		// verify initial balances
		assert_eq!(Balances::free_balance(&taker), 1000000);
		assert_eq!(Balances::free_balance(&maker), 1000000);
		assert_eq!(Balances::total_balance_on_hold(&taker), 0u128);

		// create immutables for the test
		let immutables = create_test_htlc_immutables(
			order_hash,
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
			HtlcEscrow::dst_withdraw(
				RuntimeOrigin::signed(taker),
				immutables.clone(),
				secret.to_vec(),
			),
			Error::<Test>::EarlyWithdrawal,
		);

		// Move to withdrawal period + 10 extra blocks
		let after_withdrawal_block = immutables.timelocks.withdrawal_after + 10;
		System::set_block_number(after_withdrawal_block);

		assert_ok!(HtlcEscrow::dst_withdraw(
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

		// cannot `withdraw` again
		assert_noop!(
			HtlcEscrow::dst_withdraw(
				RuntimeOrigin::signed(taker),
				immutables.clone(),
				secret.to_vec(),
			),
			Error::<Test>::HtlcNotActive
		);

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
		let hashlock = hash_of_word(secret);

		let order_hash = hash_of_word(b"order hash");

		// verify initial balances
		assert_eq!(Balances::free_balance(&maker), 1000000);
		assert_eq!(Balances::free_balance(&taker), 1000000);
		assert_eq!(Balances::total_balance_on_hold(&taker), 0u128);
		assert_eq!(Balances::free_balance(&third_party), 1000000);

		// create immutables for the test
		let immutables = create_test_htlc_immutables(
			order_hash,
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
			HtlcEscrow::dst_public_withdraw(
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
			HtlcEscrow::dst_public_withdraw(
				RuntimeOrigin::signed(third_party),
				immutables.clone(),
				secret.to_vec(),
			),
			Error::<Test>::EarlyPublicWithdrawal,
		);

		// `third_party` calls the `withdraw` and fails; only the taker
		// can call that
		assert_noop!(
			HtlcEscrow::dst_withdraw(
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
		assert_ok!(HtlcEscrow::dst_public_withdraw(
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

		// cannot `public_withdraw` again
		assert_noop!(
			HtlcEscrow::dst_public_withdraw(
				RuntimeOrigin::signed(third_party),
				immutables.clone(),
				secret.to_vec(),
			),
			Error::<Test>::HtlcNotActive
		);

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

#[test]
fn create_htlc_and_cancel_it() {
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
		let hashlock = hash_of_word(secret);

		let order_hash = hash_of_word(b"order hash");

		// verify initial balances
		assert_eq!(Balances::free_balance(&taker), 1000000);
		assert_eq!(Balances::free_balance(&maker), 1000000);
		assert_eq!(Balances::total_balance_on_hold(&taker), 0u128);

		// create immutables for the test
		let immutables = create_test_htlc_immutables(
			order_hash,
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

		// attempt early cancellation by `taker` should fail
		assert_noop!(
			HtlcEscrow::dst_cancel(RuntimeOrigin::signed(taker), immutables.clone()),
			Error::<Test>::EarlyCancellation,
		);

		// Move to withdrawal period + 10 extra blocks
		let after_withdrawal_block = immutables.timelocks.withdrawal_after + 10;
		System::set_block_number(after_withdrawal_block);

		// cancellation by `taker` should fail; still too early to cancel
		assert_noop!(
			HtlcEscrow::dst_cancel(RuntimeOrigin::signed(taker), immutables.clone()),
			Error::<Test>::EarlyCancellation,
		);

		// Move to public withdrawal period + 10 extra blocks
		let after_public_withdrawal_block = immutables.timelocks.withdrawal_after + 10;
		System::set_block_number(after_public_withdrawal_block);

		// cancellation by `taker` should fail; still too early to cancel
		assert_noop!(
			HtlcEscrow::dst_cancel(RuntimeOrigin::signed(taker), immutables.clone()),
			Error::<Test>::EarlyCancellation,
		);

		// Move to cancellation period + 10 extra blocks
		let after_cancellation_block = immutables.timelocks.cancellation_after + 10;
		System::set_block_number(after_cancellation_block);

		// cancellation by `taker` should fail; still too early to cancel
		assert_ok!(HtlcEscrow::dst_cancel(RuntimeOrigin::signed(taker), immutables.clone()));

		// `maker` should still have the same balance as before; no swap occurred
		// `taker` should still have the same balance as before; no swap occurred
		assert_eq!(Balances::free_balance(&maker), 1000000);
		assert_eq!(Balances::free_balance(&taker), 1000000);
		assert_eq!(Balances::balance_on_hold(&crate::HoldReason::SafetyDeposit.into(), &taker), 0);

		let stored_htlc = Htlcs::<Test>::get(&htlc_id).expect("HTLC id is contained; qed");
		assert_eq!(stored_htlc.status, HtlcStatus::Cancelled);

		// cannot `withdraw` after cancellation
		assert_noop!(
			HtlcEscrow::dst_withdraw(
				RuntimeOrigin::signed(taker),
				immutables.clone(),
				secret.to_vec(),
			),
			Error::<Test>::HtlcNotActive
		);

		// verify deposited event
		System::assert_last_event(Event::HtlcCancelled { htlc_id, refund_recipient: taker }.into());
	});
}

#[test]
fn create_swap_intent_and_cancel_it() {
	new_test_ext().execute_with(|| {
		// track events
		System::set_block_number(1);

		// initial setup
		let maker = ALICE;

		let src_amount = SRC_AMOUNT;
		let dst_amount = DST_AMOUNT;
		let dst_address = get_h160_addr(ALICE + 1000);
		let nonce = 0;

		let current_block = 1u64;

		let secret = b"tests_secret";
		let hashlock = hash_of_word(secret);

		// verify initial balances
		assert_eq!(Balances::free_balance(&maker), 1000000);

		// create immutables for the test
		let swap_intent = create_swap_intent(
			hashlock,
			maker,
			src_amount,
			dst_amount,
			dst_address,
			current_block + 1000,
			nonce,
		);

		// create intent
		assert_ok!(HtlcEscrow::create_swap_intent(
			RuntimeOrigin::signed(maker),
			swap_intent.clone(),
		));

		// verify reserved funds from the maker
		assert_eq!(Balances::free_balance(&maker), 1000000 - src_amount);

		// verify swap intent is stored correclty
		let intent_key = HtlcEscrow::intent_key(&maker, nonce);
		let stored_swap_intent =
			SwapIntents::<Test>::get(&intent_key).expect("Swap intent id is contained; qed");

		assert_eq!(stored_swap_intent.status, IntentStatus::Active);
		assert_eq!(stored_swap_intent.intent.hashlock, swap_intent.hashlock);
		assert_eq!(stored_swap_intent.intent.maker, swap_intent.maker);
		assert_eq!(stored_swap_intent.intent.src_amount, swap_intent.src_amount);
		assert_eq!(stored_swap_intent.intent.dst_amount, swap_intent.dst_amount);
		assert_eq!(stored_swap_intent.intent.dst_address, swap_intent.dst_address);
		assert_eq!(stored_swap_intent.intent.timeout_after_block, swap_intent.timeout_after_block);
		assert_eq!(stored_swap_intent.intent.nonce, swap_intent.nonce);

		// verify deposited event
		System::assert_last_event(
			Event::SwapIntentCreated {
				maker,
				nonce,
				src_amount,
				dst_amount,
				dst_address,
				hashlock,
			}
			.into(),
		);

		System::set_block_number(2);

		// cancellation by `taker` should fail; still too early to cancel
		assert_ok!(HtlcEscrow::cancel_swap_intent(
			RuntimeOrigin::signed(maker),
			stored_swap_intent.intent.nonce
		));

		// `maker` should still have the same balance as before; no swap occurred
		// and intent cancelled
		assert_eq!(Balances::free_balance(&maker), 1000000);

		let stored_swap_intent =
			SwapIntents::<Test>::get(&intent_key).expect("Swap intent id is contained; qed");

		assert_eq!(stored_swap_intent.status, IntentStatus::Cancelled);

		// cancelling an already cancelled intent fails
		assert_noop!(
			HtlcEscrow::cancel_swap_intent(
				RuntimeOrigin::signed(maker),
				stored_swap_intent.intent.nonce
			),
			Error::<Test>::IntentNotActive
		);

		System::assert_last_event(
			Event::SwapIntentCancelled {
				maker,
				nonce,
				src_amount,
				dst_amount,
				dst_address,
				hashlock,
			}
			.into(),
		);
	});
}

#[test]
fn create_swap_intent_then_dst_htlc_then_withdraw() {
	new_test_ext().execute_with(|| {
		// track events
		System::set_block_number(1);

		// initial setup
		let maker = ALICE;
		let taker = RESOLVER_BOB;

		let src_amount = SRC_AMOUNT;
		let dst_amount = DST_AMOUNT;
		let dst_address = get_h160_addr(ALICE + 1000);
		let nonce = 0;

		let safety_deposit = SAFETY_DEPOSIT;

		let current_block = 1u64;

		let secret = b"tests_secret";
		let hashlock = hash_of_word(secret);

		// verify initial balances
		assert_eq!(Balances::free_balance(&maker), 1000000);

		// create immutables for the test
		let swap_intent = create_swap_intent(
			hashlock,
			maker,
			src_amount,
			dst_amount,
			dst_address,
			current_block + 1000,
			nonce,
		);

		////
		// Stage 1: Create swap intent
		assert_ok!(HtlcEscrow::create_swap_intent(
			RuntimeOrigin::signed(maker),
			swap_intent.clone(),
		));

		// verify reserved funds from the maker
		assert_eq!(Balances::free_balance(&maker), 1000000 - src_amount);

		// verify swap intent is stored correclty
		let intent_key = HtlcEscrow::intent_key(&maker, nonce);
		let stored_swap_intent =
			SwapIntents::<Test>::get(&intent_key).expect("Swap intent id is contained; qed");

		assert_eq!(stored_swap_intent.status, IntentStatus::Active);
		assert_eq!(stored_swap_intent.intent.hashlock, swap_intent.hashlock);
		assert_eq!(stored_swap_intent.intent.maker, swap_intent.maker);
		assert_eq!(stored_swap_intent.intent.src_amount, swap_intent.src_amount);
		assert_eq!(stored_swap_intent.intent.dst_amount, swap_intent.dst_amount);
		assert_eq!(stored_swap_intent.intent.dst_address, swap_intent.dst_address);
		assert_eq!(stored_swap_intent.intent.timeout_after_block, swap_intent.timeout_after_block);
		assert_eq!(stored_swap_intent.intent.nonce, swap_intent.nonce);

		// verify deposited event
		System::assert_last_event(
			Event::SwapIntentCreated {
				maker,
				nonce,
				src_amount,
				dst_amount,
				dst_address,
				hashlock,
			}
			.into(),
		);

		System::set_block_number(2);

		let timelocks = create_timelocks(1);

		////
		// Stage 2: Taker creates SRC HTLC
		assert_ok!(HtlcEscrow::create_src_htlc(
			RuntimeOrigin::signed(taker),
			maker,
			nonce,
			timelocks,
			safety_deposit,
		));

		// verify reserved funds
		assert_eq!(Balances::free_balance(&taker), 1000000 - safety_deposit);
		assert_eq!(
			Balances::balance_on_hold(&crate::HoldReason::SafetyDeposit.into(), &taker),
			safety_deposit
		);
		assert_eq!(Balances::free_balance(&maker), 1000000 - src_amount);
		assert_eq!(
			Balances::balance_on_hold(&crate::HoldReason::MakerSwapIntentAmount.into(), &maker),
			src_amount
		);

		let immutables = create_test_htlc_immutables(
			intent_key,
			hashlock,
			maker,
			taker,
			src_amount,
			safety_deposit,
			1,
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
				amount: src_amount,
				safety_deposit,
			}
			.into(),
		);
	});
}

#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

use frame_support::pallet_prelude::*;
use sp_core::H160;
use sp_runtime::traits::SaturatedConversion;
use fp_rent::EvmRentCalculator;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::{dispatch::DispatchResult, traits::EnsureOrigin};
	use frame_system::{ensure_root, pallet_prelude::OriginFor};

	// ==========================================
	// 1. Configuration & Constants
	// ==========================================

	// 2026-01-01 00:00:00 UTC
	pub const DEFAULT_RENT_START_TIME: u64 = 1_767_225_600_000;
	// 2025-12-10 00:00:00 UTC
	pub const MIN_START_TIME: u64 = 1_765_324_800_000;
	// 10 satoshis = 100 Gwei (10 * 10^10)
	pub const DEFAULT_DAILY_RENT: u128 = 100_000_000_000;
	// Milliseconds per day
	pub const MILLISECONDS_PER_DAY: u64 = 86_400_000;

	#[pallet::config]
	pub trait Config: frame_system::Config + pallet_timestamp::Config {
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		/// A majority of the council can execute some transactions.
		type CouncilOrigin: EnsureOrigin<Self::Origin>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	// ==========================================
	// 2. Storage Layer
	// ==========================================

	/// System rent activation timestamp (milliseconds)
	#[pallet::storage]
	#[pallet::getter(fn active_timestamp)]
	pub type ActiveTimestamp<T: Config> = StorageValue<_, u64, ValueQuery, DefaultActiveTimestamp>;

	/// Daily rent amount (default 100 Gwei)
	#[pallet::storage]
	#[pallet::getter(fn daily_rent)]
	pub type DailyRent<T: Config> = StorageValue<_, u128, ValueQuery, DefaultDailyRent>;

	/// Account rent status structure
	#[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
	pub struct RentStatus {
		pub last_rent_paid_time: u64, // Last settlement time
		pub accumulated_rent: u128,   // Total accumulated rent (statistical purpose)
	}

	/// Core storage: H160 -> Rent status
	#[pallet::storage]
	#[pallet::getter(fn account_rent_status)]
	pub type AccountRentMap<T: Config> = StorageMap<_, Twox64Concat, H160, RentStatus, OptionQuery>;

	// Default value implementations
	pub struct DefaultActiveTimestamp;
	impl Get<u64> for DefaultActiveTimestamp {
		fn get() -> u64 {
			DEFAULT_RENT_START_TIME
		}
	}

	pub struct DefaultDailyRent;
	impl Get<u128> for DefaultDailyRent {
		fn get() -> u128 {
			DEFAULT_DAILY_RENT
		}
	}

	// ==========================================
	// 3. Events
	// ==========================================
	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// Rent charged: [Account, DaysPaid, Amount]
		RentChargedToBurn(H160, u64, u128),
	}

	#[pallet::error]
	pub enum Error<T> {
		/// Invalid timestamp
		InvalidTimestamp,
		/// Invalid daily rent
		InvalidDailyRent
	}

	// ==========================================
	// 4. Dispatchable Functions
	// ==========================================
	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(10_000 + T::DbWeight::get().writes(1))]
		pub fn set_active_timestamp(origin: OriginFor<T>, timestamp: u64) -> DispatchResult {
			<T as pallet::Config>::CouncilOrigin::try_origin(origin)
				.map(|_| ())
				.or_else(ensure_root)?;

			ensure!(timestamp >= MIN_START_TIME, Error::<T>::InvalidTimestamp);

			<ActiveTimestamp<T>>::put(timestamp);

			Ok(())
		}

		#[pallet::weight(10_000 + T::DbWeight::get().writes(1))]
		pub fn set_daily_rent(origin: OriginFor<T>, rent: u128) -> DispatchResult {
			<T as pallet::Config>::CouncilOrigin::try_origin(origin)
				.map(|_| ())
				.or_else(ensure_root)?;

			ensure!(rent % 10_000_000_000 == 0, Error::<T>::InvalidDailyRent);

			<DailyRent<T>>::put(rent);

			Ok(())
		}
	}

	// ==========================================
	// 5. Implementation
	// ==========================================

	impl<T: Config> EvmRentCalculator for Pallet<T> {
		/// Calculate and update state, return amount to charge
		/// This method should be called by EVM Adapter (OnChargeEVMTransaction)
		fn process_rent(who: H160) -> u128 {
			let now = <pallet_timestamp::Pallet<T>>::now().saturated_into::<u64>();
			let start_time: u64 = Self::active_timestamp();

			// 1. If current time is before rent start time, no charge
			if now < start_time {
				return 0;
			}

			// 2. Get user status
			let mut status = Self::account_rent_status(who).unwrap_or(RentStatus {
				last_rent_paid_time: start_time, // Default to system rent start time
				accumulated_rent: 0,
			});

			// Defensive check: Prevent time regression
			if now <= status.last_rent_paid_time {
				return 0;
			}

			// 3. Calculate elapsed time and days (floor)
			let elapsed_ms = now - status.last_rent_paid_time;
			let days_to_pay = elapsed_ms / MILLISECONDS_PER_DAY;

			// 4. Less than 1 day, no charge, no state update
			if days_to_pay == 0 {
				return 0;
			}

			// 5. Calculate amount
			let daily_rent = Self::daily_rent();
			let rent_amount = (days_to_pay as u128).saturating_mul(daily_rent);

			// 6. Update state
			// Key: Only advance paid days, keep remainder
			let time_paid_for = days_to_pay * MILLISECONDS_PER_DAY;
			status.last_rent_paid_time += time_paid_for;
			status.accumulated_rent = status.accumulated_rent.saturating_add(rent_amount);

			// 7. Write to storage
			<AccountRentMap<T>>::insert(who, status);

			// 8. Emit event
			Self::deposit_event(Event::RentChargedToBurn(who, days_to_pay, rent_amount));

			rent_amount
		}

		/// Corresponds to Solidity: estimateRent(address account)
		fn estimate_rent(who: H160) -> (u128, u64) {
			let now = <pallet_timestamp::Pallet<T>>::now().saturated_into::<u64>();
			let start_time: u64 = Self::active_timestamp();

			if now < start_time {
				return (0, 0);
			}

			let status = Self::account_rent_status(who).unwrap_or(RentStatus {
				last_rent_paid_time: start_time,
				accumulated_rent: 0,
			});

			if now <= status.last_rent_paid_time {
				return (0, 0);
			}

			let elapsed_ms = now - status.last_rent_paid_time;
			let days = elapsed_ms / MILLISECONDS_PER_DAY;

			if days == 0 {
				return (0, 0);
			}

			let rent_amount = (days as u128).saturating_mul(Self::daily_rent());

			(rent_amount, days)
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate as pallet_evm_rent;

	use frame_support::{assert_ok, parameter_types, sp_io::TestExternalities, traits::ConstU32};
	use sp_core::{H160, H256};
	use sp_runtime::{
		testing::Header,
		traits::{BlakeTwo256, IdentityLookup},
	};

	pub fn new_test_ext() -> TestExternalities {
		let t = frame_system::GenesisConfig::default()
			.build_storage::<Test>()
			.unwrap();

		// Initialize with default genesis config (skip as our pallet doesn't have GenesisConfig)

		let mut ext = TestExternalities::new(t);
		// Set initial timestamp to 2026-01-01 00:00:00 UTC
		ext.execute_with(|| {
			frame_system::Pallet::<Test>::set_block_number(1);
		});
		ext
	}

	type UncheckedExtrinsic = frame_system::mocking::MockUncheckedExtrinsic<Test>;
	type Block = frame_system::mocking::MockBlock<Test>;

	parameter_types! {
		pub const BlockHashCount: u64 = 250;
		pub BlockWeights: frame_system::limits::BlockWeights =
			frame_system::limits::BlockWeights::simple_max(1024);
	}

	impl frame_system::Config for Test {
		type BaseCallFilter = frame_support::traits::Everything;
		type BlockWeights = ();
		type BlockLength = ();
		type DbWeight = ();
		type Origin = Origin;
		type Index = u64;
		type BlockNumber = u64;
		type Call = Call;
		type Hash = H256;
		type Hashing = BlakeTwo256;
		type AccountId = u64;
		type Lookup = IdentityLookup<Self::AccountId>;
		type Header = Header;
		type Event = Event;
		type BlockHashCount = BlockHashCount;
		type Version = ();
		type PalletInfo = PalletInfo;
		type AccountData = ();
		type OnNewAccount = ();
		type OnKilledAccount = ();
		type SystemWeightInfo = ();
		type SS58Prefix = ();
		type OnSetCode = ();
		type MaxConsumers = ConstU32<16>;
	}

	impl pallet_timestamp::Config for Test {
		type Moment = u64;
		type OnTimestampSet = ();
		type WeightInfo = ();
		type MinimumPeriod = frame_support::traits::ConstU64<6000>;
	}

	frame_support::parameter_types! {
		pub const ExistentialDeposit: u128 = 0;
	}

	impl Config for Test {
		type Event = Event;
		type CouncilOrigin = frame_system::EnsureRoot<u64>;
	}

	frame_support::construct_runtime!(
		pub enum Test where
			Block = Block,
			NodeBlock = Block,
			UncheckedExtrinsic = UncheckedExtrinsic,
		{
			System: frame_system::{Pallet, Call, Config, Storage, Event<T>},
			Timestamp: pallet_timestamp::{Pallet, Call, Storage, Inherent},
			EvmRent: pallet_evm_rent::{Pallet, Call, Storage, Event<T>},
		}
	);

	#[test]
	fn test_governance_functions() {
		new_test_ext().execute_with(|| {
			let new_timestamp = DEFAULT_RENT_START_TIME + MILLISECONDS_PER_DAY;
			let new_rent = 200_000_000_000; // 200 Gwei

			assert_ok!(EvmRent::set_active_timestamp(Origin::root(), new_timestamp));
			assert_eq!(EvmRent::active_timestamp(), new_timestamp);

			assert_ok!(EvmRent::set_daily_rent(Origin::root(), new_rent));
			assert_eq!(EvmRent::daily_rent(), new_rent);
		});
	}

	#[test]
	fn test_rent_status_creation() {
		new_test_ext().execute_with(|| {
			let account = H160::from_low_u64_be(1);

			// Should have no rent status initially
			assert_eq!(EvmRent::account_rent_status(account), None);
		});
	}

	#[test]
	fn test_estimate_rent_before_start_time() {
		new_test_ext().execute_with(|| {
			let account = H160::from_low_u64_be(1);

			// Set timestamp before start time
			pallet_timestamp::Pallet::<Test>::set_timestamp(
				DEFAULT_RENT_START_TIME - MILLISECONDS_PER_DAY,
			);

			let (amount, days) = EvmRent::estimate_rent(account);
			assert_eq!(amount, 0);
			assert_eq!(days, 0);
		});
	}

	#[test]
	fn test_estimate_rent_exact_multiple_days() {
		new_test_ext().execute_with(|| {
			let account = H160::from_low_u64_be(1);

			// Set timestamp to exactly 3 days after start time
			pallet_timestamp::Pallet::<Test>::set_timestamp(
				DEFAULT_RENT_START_TIME + 3 * MILLISECONDS_PER_DAY,
			);

			let (amount, days) = EvmRent::estimate_rent(account);
			assert_eq!(amount, DEFAULT_DAILY_RENT * 3);
			assert_eq!(days, 3);
		});
	}

	#[test]
	fn test_estimate_rent_partial_day() {
		new_test_ext().execute_with(|| {
			let account = H160::from_low_u64_be(1);

			// Set timestamp to 2 days + 12 hours after start time
			pallet_timestamp::Pallet::<Test>::set_timestamp(
				DEFAULT_RENT_START_TIME + 2 * MILLISECONDS_PER_DAY + 43200_000,
			);

			// Should only charge for 2 complete days
			let (amount, days) = EvmRent::estimate_rent(account);
			assert_eq!(amount, DEFAULT_DAILY_RENT * 2);
			assert_eq!(days, 2);
		});
	}

	#[test]
	fn test_estimate_rent_less_than_one_day() {
		new_test_ext().execute_with(|| {
			let account = H160::from_low_u64_be(1);

			// Set timestamp to 12 hours after start time
			pallet_timestamp::Pallet::<Test>::set_timestamp(DEFAULT_RENT_START_TIME + 43200_000);

			let (amount, days) = EvmRent::estimate_rent(account);
			assert_eq!(amount, 0);
			assert_eq!(days, 0);
		});
	}

	#[test]
	fn test_process_rent_creates_account_status() {
		new_test_ext().execute_with(|| {
			let account = H160::from_low_u64_be(1);

			// Set timestamp to 2 days after start time
			pallet_timestamp::Pallet::<Test>::set_timestamp(
				DEFAULT_RENT_START_TIME + 2 * MILLISECONDS_PER_DAY,
			);

			// Process rent
			let rent_amount = EvmRent::process_rent(account);
			assert_eq!(rent_amount, DEFAULT_DAILY_RENT * 2);

			// Check that account status was created
			let status = EvmRent::account_rent_status(account).unwrap();
			assert_eq!(
				status.last_rent_paid_time,
				DEFAULT_RENT_START_TIME + 2 * MILLISECONDS_PER_DAY
			);
			assert_eq!(status.accumulated_rent, DEFAULT_DAILY_RENT * 2);
		});
	}

	#[test]
	fn test_process_rent_no_duplicate_charging() {
		new_test_ext().execute_with(|| {
			let account = H160::from_low_u64_be(1);

			// Set timestamp to 3 days after start time
			pallet_timestamp::Pallet::<Test>::set_timestamp(
				DEFAULT_RENT_START_TIME + 3 * MILLISECONDS_PER_DAY,
			);

			// First call should charge rent
			let rent_amount1 = EvmRent::process_rent(account);
			assert_eq!(rent_amount1, DEFAULT_DAILY_RENT * 3);

			// Second call immediately should return 0 (no new days passed)
			let rent_amount2 = EvmRent::process_rent(account);
			assert_eq!(rent_amount2, 0);

			// Status should remain unchanged after second call
			let status = EvmRent::account_rent_status(account).unwrap();
			assert_eq!(status.accumulated_rent, DEFAULT_DAILY_RENT * 3);
		});
	}

	#[test]
	fn test_process_rent_cumulative() {
		new_test_ext().execute_with(|| {
			let account = H160::from_low_u64_be(1);

			// First period: 2 days
			pallet_timestamp::Pallet::<Test>::set_timestamp(
				DEFAULT_RENT_START_TIME + 2 * MILLISECONDS_PER_DAY,
			);
			let rent_amount1 = EvmRent::process_rent(account);
			assert_eq!(rent_amount1, DEFAULT_DAILY_RENT * 2);

			// Second period: advance to 5 total days (3 more days)
			pallet_timestamp::Pallet::<Test>::set_timestamp(
				DEFAULT_RENT_START_TIME + 5 * MILLISECONDS_PER_DAY,
			);
			let rent_amount2 = EvmRent::process_rent(account);
			assert_eq!(rent_amount2, DEFAULT_DAILY_RENT * 3);

			// Total accumulated rent should be 5 days
			let status = EvmRent::account_rent_status(account).unwrap();
			assert_eq!(status.accumulated_rent, DEFAULT_DAILY_RENT * 5);
			assert_eq!(
				status.last_rent_paid_time,
				DEFAULT_RENT_START_TIME + 5 * MILLISECONDS_PER_DAY
			);
		});
	}

	#[test]
	fn test_process_rent_with_custom_daily_rent() {
		new_test_ext().execute_with(|| {
			let account = H160::from_low_u64_be(1);
			let custom_rent = 150_000_000_000; // 150 Gwei

			// Set custom daily rent
			assert_ok!(EvmRent::set_daily_rent(Origin::root(), custom_rent));

			// Set timestamp to 2 days after start time
			pallet_timestamp::Pallet::<Test>::set_timestamp(
				DEFAULT_RENT_START_TIME + 2 * MILLISECONDS_PER_DAY,
			);

			let rent_amount = EvmRent::process_rent(account);
			assert_eq!(rent_amount, custom_rent * 2);

			let status = EvmRent::account_rent_status(account).unwrap();
			assert_eq!(status.accumulated_rent, custom_rent * 2);
		});
	}

	#[test]
	fn test_process_rent_with_custom_start_time() {
		new_test_ext().execute_with(|| {
			let account = H160::from_low_u64_be(1);
			let custom_start_time = DEFAULT_RENT_START_TIME + MILLISECONDS_PER_DAY; // +1 day

			// Set custom start time
			assert_ok!(EvmRent::set_active_timestamp(
				Origin::root(),
				custom_start_time
			));

			// Set timestamp to 2 days after custom start time
			pallet_timestamp::Pallet::<Test>::set_timestamp(
				custom_start_time + 2 * MILLISECONDS_PER_DAY,
			);

			let rent_amount = EvmRent::process_rent(account);
			assert_eq!(rent_amount, DEFAULT_DAILY_RENT * 2);

			let status = EvmRent::account_rent_status(account).unwrap();
			assert_eq!(
				status.last_rent_paid_time,
				custom_start_time + 2 * MILLISECONDS_PER_DAY
			);
			assert_eq!(status.accumulated_rent, DEFAULT_DAILY_RENT * 2);
		});
	}

	#[test]
	fn test_zero_daily_rent() {
		new_test_ext().execute_with(|| {
			let account = H160::from_low_u64_be(1);

			// Set daily rent to 0
			assert_ok!(EvmRent::set_daily_rent(Origin::root(), 0));

			// Set timestamp to 5 days after start time
			pallet_timestamp::Pallet::<Test>::set_timestamp(
				DEFAULT_RENT_START_TIME + 5 * MILLISECONDS_PER_DAY,
			);

			let (estimate_amount, estimate_days) = EvmRent::estimate_rent(account);
			assert_eq!(estimate_amount, 0);
			assert_eq!(estimate_days, 5);

			let process_amount = EvmRent::process_rent(account);
			assert_eq!(process_amount, 0);
		});
	}

	#[test]
	fn test_multiple_accounts_independent() {
		new_test_ext().execute_with(|| {
			let account1 = H160::from_low_u64_be(1);
			let account2 = H160::from_low_u64_be(2);

			// Set timestamp to 3 days after start time
			pallet_timestamp::Pallet::<Test>::set_timestamp(
				DEFAULT_RENT_START_TIME + 3 * MILLISECONDS_PER_DAY,
			);

			// Process rent for both accounts
			let rent_amount1 = EvmRent::process_rent(account1);
			let rent_amount2 = EvmRent::process_rent(account2);
			assert_eq!(rent_amount1, DEFAULT_DAILY_RENT * 3);
			assert_eq!(rent_amount2, DEFAULT_DAILY_RENT * 3);

			// Check that both accounts have independent status
			let status1 = EvmRent::account_rent_status(account1).unwrap();
			let status2 = EvmRent::account_rent_status(account2).unwrap();

			assert_eq!(status1.accumulated_rent, DEFAULT_DAILY_RENT * 3);
			assert_eq!(status2.accumulated_rent, DEFAULT_DAILY_RENT * 3);
			assert_eq!(status1.last_rent_paid_time, status2.last_rent_paid_time);
		});
	}

	#[test]
	fn test_set_daily_rent_with_zero_amount() {
		new_test_ext().execute_with(|| {
			// Zero rent should be allowed (flexibility for free tier, testing, etc.)
			assert_ok!(EvmRent::set_daily_rent(Origin::root(), 0));
			assert_eq!(EvmRent::daily_rent(), 0);
		});
	}

	#[test]
	fn test_set_daily_rent_with_invalid_amount() {
		new_test_ext().execute_with(|| {
			// Non-multiple of 10 Gwei should be rejected
			let invalid_rent = 123_456_789; // Not a multiple of 10_000_000_000
			let result = EvmRent::set_daily_rent(Origin::root(), invalid_rent);
			assert!(result.is_err());

			// Verify the rent wasn't changed
			assert_eq!(EvmRent::daily_rent(), DEFAULT_DAILY_RENT);
		});
	}

	#[test]
	fn test_set_daily_rent_with_valid_amounts() {
		new_test_ext().execute_with(|| {
			// Test various valid amounts (multiples of 10 Gwei)
			let valid_amounts = vec![
				0,                    // Zero rent (allowed)
				10_000_000_000,       // 10 Gwei
				100_000_000_000,      // 100 Gwei (default)
				1_000_000_000_000,    // 1000 Gwei
			];

			for amount in valid_amounts {
				assert_ok!(EvmRent::set_daily_rent(Origin::root(), amount));
				assert_eq!(EvmRent::daily_rent(), amount);
			}
		});
	}

	#[test]
	fn test_set_active_timestamp_validation() {
		new_test_ext().execute_with(|| {
			// Test valid timestamp (milliseconds)
			let valid_timestamp = DEFAULT_RENT_START_TIME + MILLISECONDS_PER_DAY;
			assert_ok!(EvmRent::set_active_timestamp(Origin::root(), valid_timestamp));
			assert_eq!(EvmRent::active_timestamp(), valid_timestamp);
		});
	}
}

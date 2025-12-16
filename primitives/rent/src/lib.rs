//! # EVM Rent
#![cfg_attr(not(feature = "std"), no_std)]

use sp_core::H160;

pub trait EvmRentCalculator {
	/// Calculate and charge (burn) the outstanding rent for an account,
	/// then update its status map.
	fn process_rent(who: H160) -> u128;
	/// Estimate the amount of rent to be burned and the corresponding
	/// number of rented days for an account.
	fn estimate_rent(who: H160) -> (u128, u64);
}

impl EvmRentCalculator for () {
	fn process_rent(_who: H160) -> u128 {
		0u128
	}

	fn estimate_rent(_who: H160) -> (u128, u64) {
		(0u128, 0u64)
	}
}
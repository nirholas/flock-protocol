//! Fixed-point helpers. Every one of them is checked, and every one of them rounds in the
//! direction that favors the fund rather than the caller.
//!
//! The rule is simple and load-bearing: what a caller must deliver rounds up, what a caller
//! receives rounds down. Applied consistently, the vault balances can only ever drift upward
//! relative to what the outstanding supply is entitled to, which is what keeps the backing
//! invariant true after millions of issuances.

use crate::error::FlockError;

/// Component units are quoted per whole index token, and the index mint has 9 decimals.
pub const UNIT_SCALE: u128 = 1_000_000_000;
pub const BPS: u128 = 10_000;
pub const SECONDS_PER_YEAR: u128 = 31_536_000;

pub fn mul_div_floor(a: u128, b: u128, d: u128) -> Result<u128, FlockError> {
    if d == 0 {
        return Err(FlockError::MathOverflow);
    }
    a.checked_mul(b).ok_or(FlockError::MathOverflow).map(|x| x / d)
}

pub fn mul_div_ceil(a: u128, b: u128, d: u128) -> Result<u128, FlockError> {
    if d == 0 {
        return Err(FlockError::MathOverflow);
    }
    let n = a.checked_mul(b).ok_or(FlockError::MathOverflow)?;
    Ok(n.div_ceil(d))
}

pub fn to_u64(x: u128) -> Result<u64, FlockError> {
    u64::try_from(x).map_err(|_| FlockError::MathOverflow)
}

/// Component base units a caller must deliver to mint `amount` index base units.
pub fn units_in(units: u64, amount: u64) -> Result<u64, FlockError> {
    to_u64(mul_div_ceil(units as u128, amount as u128, UNIT_SCALE)?)
}

/// Component base units a caller receives for burning `amount` index base units.
pub fn units_out(units: u64, amount: u64) -> Result<u64, FlockError> {
    to_u64(mul_div_floor(units as u128, amount as u128, UNIT_SCALE)?)
}

/// The units a component is worth given what the vault actually holds. Deriving units from
/// balances rather than tracking them incrementally is what makes rounding dust harmless: it
/// accrues to holders instead of accumulating into an unbacked claim.
pub fn units_from_balance(balance: u64, supply: u64) -> Result<u64, FlockError> {
    if supply == 0 {
        return Err(FlockError::SupplyTooSmall);
    }
    to_u64(mul_div_floor(balance as u128, UNIT_SCALE, supply as u128)?)
}

/// USD value in 1e9 of `balance` base units of a token with `decimals` decimals, priced at
/// `price_e9` USD per whole token.
pub fn value_e9(balance: u64, decimals: u8, price_e9: u64) -> Result<u128, FlockError> {
    let scale = 10u128.checked_pow(decimals as u32).ok_or(FlockError::MathOverflow)?;
    mul_div_floor(balance as u128, price_e9 as u128, scale)
}

/// Base units of a token worth `value` USD-e9, rounded up.
pub fn amount_from_value_ceil(value: u128, decimals: u8, price_e9: u64) -> Result<u64, FlockError> {
    if price_e9 == 0 {
        return Err(FlockError::InvalidPrice);
    }
    let scale = 10u128.checked_pow(decimals as u32).ok_or(FlockError::MathOverflow)?;
    to_u64(mul_div_ceil(value, scale, price_e9 as u128)?)
}

/// Streaming fee, charged as dilution.
///
/// Over `dt` seconds an annual rate `f` should leave the fee recipient holding `f * dt / year`
/// of the fund. Minting exactly that fraction of the *old* supply undercharges, because the mint
/// itself grows the denominator, so the amount minted is grossed up by the same fraction. This is
/// the identical construction Set Protocol's StreamingFeeModule uses, and it is why the fee is
/// exact rather than approximately right.
pub fn streaming_fee_tokens(supply: u64, fee_bps: u16, dt: i64) -> Result<u64, FlockError> {
    if supply == 0 || fee_bps == 0 || dt <= 0 {
        return Ok(0);
    }
    let num = (fee_bps as u128).checked_mul(dt as u128).ok_or(FlockError::MathOverflow)?;
    let den = BPS.checked_mul(SECONDS_PER_YEAR).ok_or(FlockError::MathOverflow)?;
    if num >= den {
        return Err(FlockError::MathOverflow);
    }
    to_u64(mul_div_floor(supply as u128, num, den - num)?)
}

/// The auction discount in force at `now`, decaying linearly from `start_bps` to `end_bps`.
///
/// Negative means the bidder overpays the fund, positive means the bidder is paid to take the
/// trade. An auction opens negative and decays; the clearing point is wherever a bidder's own
/// execution cost is covered, which is how the fund discovers the real price of its rebalance
/// instead of assuming an oracle already knows it.
pub fn premium_at(start_bps: i16, end_bps: i16, start_ts: i64, end_ts: i64, now: i64) -> Result<i32, FlockError> {
    if now < start_ts || now > end_ts {
        return Err(FlockError::AuctionClosed);
    }
    if end_ts <= start_ts {
        return Err(FlockError::InvalidPremium);
    }
    let span = (end_ts - start_ts) as i128;
    let elapsed = (now - start_ts) as i128;
    let delta = (end_bps as i128 - start_bps as i128) * elapsed / span;
    i32::try_from(start_bps as i128 + delta).map_err(|_| FlockError::MathOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caller_deliveries_round_up_and_payouts_round_down() {
        // 1 base unit of a component per index token, redeeming a third of a token.
        assert_eq!(units_in(1, 333_333_333).unwrap(), 1);
        assert_eq!(units_out(1, 333_333_333).unwrap(), 0);
    }

    #[test]
    fn units_never_overstate_the_vault() {
        for (balance, supply) in [(1_000_000u64, 3_000_000u64), (7, 3), (u64::MAX / 2, 1_000_000_000)] {
            let units = units_from_balance(balance, supply).unwrap();
            let claim = mul_div_floor(units as u128, supply as u128, UNIT_SCALE).unwrap();
            assert!(claim <= balance as u128, "units {units} overclaim {balance}");
        }
    }

    #[test]
    fn streaming_fee_gives_the_recipient_its_exact_share() {
        // 100 bps for a full year on a 1_000e9 supply: the recipient must end up with 1% of the
        // grown supply, not 1% of the old one.
        let supply = 1_000_000_000_000u64;
        let minted = streaming_fee_tokens(supply, 100, SECONDS_PER_YEAR as i64).unwrap();
        let new_supply = supply + minted;
        // Measured in ninths of a bp so the floor in the mint is visible rather than hidden by a
        // coarse assertion: the recipient must land just under 100 bps and never over it.
        let share_e9 = (minted as u128) * 1_000_000_000 / (new_supply as u128);
        let target_e9 = 100 * 1_000_000_000 / BPS;
        assert!(share_e9 <= target_e9, "fee overcharged: {share_e9} > {target_e9}");
        assert!(target_e9 - share_e9 <= 1, "fee undercharged by more than rounding: {share_e9}");
    }

    #[test]
    fn zero_fee_and_zero_elapsed_are_free() {
        assert_eq!(streaming_fee_tokens(1_000, 0, 10_000).unwrap(), 0);
        assert_eq!(streaming_fee_tokens(1_000, 100, 0).unwrap(), 0);
    }

    #[test]
    fn premium_decays_linearly_and_is_closed_outside_the_window() {
        assert_eq!(premium_at(-50, 150, 1_000, 2_000, 1_000).unwrap(), -50);
        assert_eq!(premium_at(-50, 150, 1_000, 2_000, 1_500).unwrap(), 50);
        assert_eq!(premium_at(-50, 150, 1_000, 2_000, 2_000).unwrap(), 150);
        assert_eq!(premium_at(-50, 150, 1_000, 2_000, 999), Err(FlockError::AuctionClosed));
        assert_eq!(premium_at(-50, 150, 1_000, 2_000, 2_001), Err(FlockError::AuctionClosed));
    }
}

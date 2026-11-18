use crate::world::World;
use alloy::primitives::{Address, U256};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod serde_u256_dec_str {
    use alloy::primitives::U256;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &U256, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<U256, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        U256::from_str_radix(&s, 10).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EligibilityRequest {
    pub address: Address,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EligibilityResponse {
    pub address: Address,
    pub tokens: Vec<EligibleToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EligibleToken {
    pub claim_index: u64,
    pub token_address: Address,
    pub symbol: String,

    /// Human-friendly balance for display only. Consumers should prefer
    /// `raw_balance` + `decimals` when doing any real accounting.
    pub display_balance: String,

    #[serde(with = "serde_u256_dec_str")]
    pub raw_balance: U256,
    pub decimals: u8,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ActiveEligibilityRow {
    claim_index: i64,
    token_address: String,
    amount: String,
    symbol: String,
    decimals: i16,
}

#[derive(Debug, Error)]
pub enum EligibilityError {
    #[error("storage error")]
    Storage { detail: String },
}

impl EligibilityError {
    pub(crate) fn storage(detail: impl std::fmt::Display) -> Self {
        Self::Storage {
            detail: detail.to_string(),
        }
    }
}

pub async fn check_eligibility(
    world: &World,
    req: EligibilityRequest,
) -> Result<EligibilityResponse, EligibilityError> {
    let active_rows = load_active_snapshot_rows(world, req.address).await?;
    let mut tokens = Vec::with_capacity(active_rows.len());

    for row in active_rows {
        if let Some(token) = eligible_token_from_row(row)? {
            tokens.push(token);
        }
    }

    Ok(EligibilityResponse {
        address: req.address,
        tokens,
    })
}

fn eligible_token_from_row(
    row: ActiveEligibilityRow,
) -> Result<Option<EligibleToken>, EligibilityError> {
    let token_address: Address = row
        .token_address
        .parse()
        .map_err(|_| EligibilityError::storage("invalid token_address in eligibility query"))?;
    let raw_balance = U256::from_str_radix(&row.amount, 10)
        .map_err(|_| EligibilityError::storage("invalid amount in eligibility query"))?;
    if raw_balance.is_zero() {
        return Ok(None);
    }

    let decimals = u8::try_from(row.decimals)
        .map_err(|_| EligibilityError::storage("invalid decimals in snapshot_tokens"))?;
    let claim_index = u64::try_from(row.claim_index).map_err(|_| {
        EligibilityError::storage(format!(
            "invalid claim_index in eligibility query: {}",
            row.claim_index
        ))
    })?;

    Ok(Some(EligibleToken {
        claim_index,
        token_address,
        symbol: row.symbol,
        display_balance: format_display_balance(raw_balance, decimals),
        raw_balance,
        decimals,
    }))
}

async fn load_active_snapshot_rows(
    world: &World,
    user: Address,
) -> Result<Vec<ActiveEligibilityRow>, EligibilityError> {
    sqlx::query_as::<_, ActiveEligibilityRow>(
        r#"
        SELECT
            ce.claim_index,
            ce.token_address,
            ce.amount,
            st.symbol,
            st.decimals
        FROM claim_entries ce
        INNER JOIN snapshot_tokens st
            ON LOWER(st.address) = LOWER(ce.token_address)
        WHERE ce.claimant_address = $1
          AND st.id < 1000
        ORDER BY ce.token_address ASC
        "#,
    )
    .bind(format!("{user:#x}"))
    .fetch_all(world.db_pool())
    .await
    .map_err(|err| {
        EligibilityError::storage(format!(
            "failed to load active snapshot rows for eligibility: {err}"
        ))
    })
}

fn format_display_balance(raw_balance: U256, decimals: u8) -> String {
    let raw = raw_balance.to_string();
    if raw == "0" {
        return "0.00".to_string();
    }

    if decimals == 0 {
        return format!("{raw}.00");
    }

    let decimals = decimals as usize;
    if is_at_least_one_hundredth(&raw, decimals) {
        return format_fixed_decimal(&raw, decimals);
    }

    let display_decimals = decimals.min(8);
    if is_below_display_floor(&raw, decimals, display_decimals) {
        return format!("<{}", format_display_floor(display_decimals));
    }

    format_trimmed_decimal(&raw, decimals, display_decimals)
}

fn is_at_least_one_hundredth(raw: &str, decimals: usize) -> bool {
    decimals < 2 || raw.len() > decimals - 2
}

fn is_below_display_floor(raw: &str, decimals: usize, display_decimals: usize) -> bool {
    raw.len() <= decimals - display_decimals
}

fn decimal_digits(raw: &str, decimals: usize) -> String {
    if raw.len() <= decimals {
        let mut padded = String::with_capacity(decimals + 1);
        padded.push('0');
        padded.extend(std::iter::repeat_n('0', decimals - raw.len()));
        padded.push_str(raw);
        padded
    } else {
        raw.to_string()
    }
}

fn format_fixed_decimal(raw: &str, decimals: usize) -> String {
    let digits = decimal_digits(raw, decimals);
    let split_at = digits.len().saturating_sub(decimals);
    let whole = &digits[..split_at];
    let fractional = &digits[split_at..];
    let fractional_two = if fractional.len() >= 2 {
        fractional[..2].to_string()
    } else if fractional.len() == 1 {
        format!("{fractional}0")
    } else {
        "00".to_string()
    };

    format!("{whole}.{fractional_two}")
}

fn format_trimmed_decimal(raw: &str, decimals: usize, display_decimals: usize) -> String {
    let digits = decimal_digits(raw, decimals);
    let split_at = digits.len().saturating_sub(decimals);
    let whole = &digits[..split_at];
    let fractional = &digits[split_at..];
    let visible = fractional[..fractional.len().min(display_decimals)].trim_end_matches('0');

    if visible.is_empty() {
        whole.to_string()
    } else {
        format!("{whole}.{visible}")
    }
}

fn format_display_floor(display_decimals: usize) -> String {
    if display_decimals == 0 {
        "1".to_string()
    } else {
        format!("0.{}1", "0".repeat(display_decimals - 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(value: &str) -> U256 {
        U256::from_str_radix(value, 10).expect("test balance should parse")
    }

    fn row(amount: &str) -> ActiveEligibilityRow {
        ActiveEligibilityRow {
            claim_index: 7,
            token_address: "0x0000000000000000000000000000000000000000".to_string(),
            amount: amount.to_string(),
            symbol: "ETH".to_string(),
            decimals: 18,
        }
    }

    #[test]
    fn eligibility_row_with_zero_balance_is_hidden() {
        let token = eligible_token_from_row(row("0")).expect("zero row should parse");

        assert_eq!(token, None);
    }

    #[test]
    fn eligibility_row_with_positive_balance_is_returned() {
        let token = eligible_token_from_row(row("10000000000000"))
            .expect("positive row should parse")
            .expect("positive row should be visible");

        assert_eq!(token.claim_index, 7);
        assert_eq!(token.symbol, "ETH");
        assert_eq!(token.raw_balance, raw("10000000000000"));
        assert_eq!(token.display_balance, "0.00001");
    }

    #[test]
    fn display_balance_uses_two_decimals_for_regular_values() {
        assert_eq!(
            format_display_balance(raw("12345678900000000000"), 18),
            "12.34"
        );
        assert_eq!(
            format_display_balance(raw("123450000000000000"), 18),
            "0.12"
        );
        assert_eq!(format_display_balance(raw("12345000000000000"), 18), "0.01");
        assert_eq!(format_display_balance(raw("12"), 0), "12.00");
        assert_eq!(format_display_balance(raw("1"), 1), "0.10");
    }

    #[test]
    fn display_balance_expands_small_non_zero_values() {
        assert_eq!(format_display_balance(raw("10000000000000"), 18), "0.00001");
        assert_eq!(format_display_balance(raw("10000000000"), 18), "0.00000001");
        assert_eq!(format_display_balance(raw("1"), 8), "0.00000001");
        assert_eq!(format_display_balance(raw("1"), 6), "0.000001");
    }

    #[test]
    fn display_balance_uses_floor_for_smaller_dust() {
        assert_eq!(format_display_balance(raw("1230000000"), 18), "<0.00000001");
        assert_eq!(format_display_balance(raw("0"), 18), "0.00");
    }
}

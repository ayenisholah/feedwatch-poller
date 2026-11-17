//! Encode and decode the two calls Feedwatch makes on a Chainlink-compatible feed:
//! `latestRoundData()` (`0xfeaf968c`) and `decimals()` (`0x313ce567`).
//!
//! Part of FR-FW-2. Pure: no I/O. Results arrive as the `0x…` hex string an `eth_call` returns.

/// Call data for `latestRoundData()`. Returns `(uint80, int256, uint256, uint256, uint80)`.
pub const LATEST_ROUND_DATA: &str = "0xfeaf968c";
/// Call data for `decimals()`. Returns `uint8`.
pub const DECIMALS: &str = "0x313ce567";

const WORD: usize = 32;

/// A feed's latest round, decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Round {
    /// The round ID (`uint80`).
    pub round_id: u128,
    /// The raw answer before scaling by `decimals`. Negative answers are rejected, since prices
    /// can't be negative, so this is unsigned.
    pub answer: u128,
    /// When the answer was last updated, in Unix seconds.
    pub updated_at: u64,
}

impl Round {
    /// The answer scaled by the feed's decimals, for display and comparison.
    pub fn scaled(&self, decimals: u8) -> f64 {
        self.answer as f64 / 10f64.powi(i32::from(decimals))
    }

    /// The raw answer as a decimal string, stored alongside the float so no precision is lost
    /// (feedwatch-platform `docs/DATA_CONTRACT.md`).
    pub fn answer_raw(&self) -> String {
        self.answer.to_string()
    }
}

/// Why a call result couldn't be decoded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AbiError {
    /// The result isn't `0x` followed by an even number of hex digits.
    #[error("result is not hex")]
    NotHex,
    /// The result has the wrong number of 32-byte words.
    #[error("expected {expected} words, got {bytes} bytes")]
    WrongLength {
        /// Words the call returns.
        expected: usize,
        /// Bytes received.
        bytes: usize,
    },
    /// The answer is negative, which a price feed must never return.
    #[error("answer is negative")]
    NegativeAnswer,
    /// A value is larger than Feedwatch stores for that field.
    #[error("{0} is out of range")]
    OutOfRange(&'static str),
}

/// Decodes a `latestRoundData()` result.
pub fn decode_round(result: &str) -> Result<Round, AbiError> {
    let bytes = decode_hex(result)?;
    let words = split_words(&bytes, 5)?;
    if words[1][0] & 0x80 != 0 {
        return Err(AbiError::NegativeAnswer);
    }
    Ok(Round {
        round_id: to_u128(words[0], "round ID")?,
        answer: to_u128(words[1], "answer")?,
        updated_at: u64::try_from(to_u128(words[3], "updatedAt")?)
            .map_err(|_| AbiError::OutOfRange("updatedAt"))?,
    })
}

/// Decodes a `decimals()` result.
pub fn decode_decimals(result: &str) -> Result<u8, AbiError> {
    let bytes = decode_hex(result)?;
    let words = split_words(&bytes, 1)?;
    u8::try_from(to_u128(words[0], "decimals")?).map_err(|_| AbiError::OutOfRange("decimals"))
}

fn decode_hex(text: &str) -> Result<Vec<u8>, AbiError> {
    let digits = text.strip_prefix("0x").ok_or(AbiError::NotHex)?.as_bytes();
    if digits.len() % 2 != 0 {
        return Err(AbiError::NotHex);
    }
    digits
        .chunks(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16).ok_or(AbiError::NotHex)?;
            let low = (pair[1] as char).to_digit(16).ok_or(AbiError::NotHex)?;
            Ok((high * 16 + low) as u8)
        })
        .collect()
}

fn split_words(bytes: &[u8], expected: usize) -> Result<Vec<&[u8]>, AbiError> {
    if bytes.len() != expected * WORD {
        return Err(AbiError::WrongLength {
            expected,
            bytes: bytes.len(),
        });
    }
    Ok(bytes.chunks(WORD).collect())
}

/// A big-endian 32-byte word as u128, if its top 16 bytes are zero.
fn to_u128(word: &[u8], field: &'static str) -> Result<u128, AbiError> {
    let (high, low) = word.split_at(16);
    if high.iter().any(|&b| b != 0) {
        return Err(AbiError::OutOfRange(field));
    }
    Ok(low.iter().fold(0u128, |acc, &b| (acc << 8) | u128::from(b)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(value: u128) -> String {
        format!("{value:064x}")
    }

    fn round_result(answer_word: &str, updated_at_word: &str) -> String {
        format!(
            "0x{}{answer_word}{}{updated_at_word}{}",
            word(7),
            word(1),
            word(7)
        )
    }

    #[test]
    fn fr_fw_2_decodes_a_round() {
        let round =
            decode_round(&round_result(&word(180_633_950_235), &word(1_794_907_199))).unwrap();
        assert_eq!(
            round,
            Round {
                round_id: 7,
                answer: 180_633_950_235,
                updated_at: 1_794_907_199
            }
        );
        assert_eq!(round.answer_raw(), "180633950235");
        assert!((round.scaled(8) - 1_806.339_502_35).abs() < 1e-9);
    }

    #[test]
    fn fr_fw_2_decodes_decimals() {
        assert_eq!(decode_decimals(&format!("0x{}", word(8))), Ok(8));
        assert_eq!(
            decode_decimals(&format!("0x{}", word(256))),
            Err(AbiError::OutOfRange("decimals"))
        );
    }

    #[test]
    fn fr_fw_2_rejects_a_negative_answer() {
        let minus_one = "f".repeat(64);
        assert_eq!(
            decode_round(&round_result(&minus_one, &word(1))),
            Err(AbiError::NegativeAnswer)
        );
    }

    #[test]
    fn fr_fw_2_rejects_values_too_large_to_store() {
        let huge = format!("{}{}", "0".repeat(31), "1".repeat(33));
        assert_eq!(
            decode_round(&round_result(&huge, &word(1))),
            Err(AbiError::OutOfRange("answer"))
        );
        let late = word(u128::from(u64::MAX) + 1);
        assert_eq!(
            decode_round(&round_result(&word(1), &late)),
            Err(AbiError::OutOfRange("updatedAt"))
        );
        let big_round = format!(
            "0x{}{}",
            "1".repeat(64),
            &round_result(&word(1), &word(1))[66..]
        );
        assert_eq!(
            decode_round(&big_round),
            Err(AbiError::OutOfRange("round ID"))
        );
    }

    #[test]
    fn fr_fw_2_rejects_malformed_results() {
        assert_eq!(decode_round("feaf"), Err(AbiError::NotHex));
        assert_eq!(decode_round("0xabc"), Err(AbiError::NotHex));
        assert_eq!(decode_round("0xzz"), Err(AbiError::NotHex));
        assert_eq!(
            decode_round("0x"),
            Err(AbiError::WrongLength {
                expected: 5,
                bytes: 0
            })
        );
        assert_eq!(
            decode_round(&format!("0x{}", word(1).repeat(4))),
            Err(AbiError::WrongLength {
                expected: 5,
                bytes: 128
            })
        );
        assert_eq!(
            AbiError::WrongLength {
                expected: 5,
                bytes: 0
            }
            .to_string(),
            "expected 5 words, got 0 bytes"
        );
    }

    #[test]
    fn call_data_selectors() {
        assert_eq!(LATEST_ROUND_DATA, "0xfeaf968c");
        assert_eq!(DECIMALS, "0x313ce567");
    }
}

//! `InstrumentId` — normalized, venue-aware instrument identity. Hand-
//! transcribed from
//! `market-system-contracts/schemas/2.0.0/instrument-id.schema.json`, the
//! same way `market_intelligence_core` transcribes its own schemas.

use serde::{Deserialize, Serialize};

use crate::decimal::Decimal;
use crate::utc_timestamp::UtcTimestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssetClass {
    Equity,
    Etf,
    Fund,
    Bond,
    Option,
    Future,
    FutureOption,
    FxSpot,
    FxForward,
    CryptoSpot,
    CryptoPerpetual,
    CryptoFuture,
    Commodity,
    Index,
    Cfd,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OptionRight {
    Call,
    Put,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExerciseStyle {
    American,
    European,
    Bermudan,
    Other,
}

/// A display symbol never uniquely identifies an instrument; duplicate
/// tickers across venues must not collide — `instrument_id` is the actual
/// identity field, `symbol` is display-only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct InstrumentId {
    pub schema_version: String,
    pub instrument_id: String,
    pub asset_class: AssetClass,
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub venue_mic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listing_jurisdiction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_currency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_currency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_currency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub figi: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiry: Option<UtcTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strike: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_right: Option<OptionRight>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exercise_style: Option<ExerciseStyle>,
    #[serde(default = "default_contract_multiplier")]
    pub contract_multiplier: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tick_size: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lot_size: Option<Decimal>,
}

fn default_contract_multiplier() -> Decimal {
    Decimal::from_i64(1)
}

impl InstrumentId {
    /// A minimal constructor for the common equity/ETF case, used
    /// throughout this crate's tests and the paper simulator's built-in
    /// fixture instruments.
    pub fn equity(instrument_id: &str, symbol: &str) -> Self {
        InstrumentId {
            schema_version: "2.0.0".to_string(),
            instrument_id: instrument_id.to_string(),
            asset_class: AssetClass::Equity,
            symbol: symbol.to_string(),
            venue_mic: None,
            listing_jurisdiction: None,
            primary_currency: Some("USD".to_string()),
            base_currency: None,
            quote_currency: None,
            isin: None,
            figi: None,
            expiry: None,
            strike: None,
            option_right: None,
            exercise_style: None,
            contract_multiplier: Decimal::from_i64(1),
            tick_size: None,
            lot_size: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equity_constructor_defaults_to_unit_multiplier() {
        let inst = InstrumentId::equity("inst-1", "SPY");
        assert_eq!(inst.contract_multiplier.to_decimal_string(), "1");
        assert_eq!(inst.asset_class, AssetClass::Equity);
    }

    #[test]
    fn serde_uses_kebab_case_field_names() {
        let inst = InstrumentId::equity("inst-1", "SPY");
        let json = serde_json::to_value(&inst).unwrap();
        assert!(json.get("instrument-id").is_some());
        assert!(json.get("asset-class").is_some());
        assert_eq!(json["asset-class"], "equity");
    }

    #[test]
    fn missing_contract_multiplier_defaults_to_one() {
        let json = r#"{
            "schema-version": "2.0.0",
            "instrument-id": "inst-2",
            "asset-class": "equity",
            "symbol": "QQQ"
        }"#;
        let inst: InstrumentId = serde_json::from_str(json).unwrap();
        assert_eq!(inst.contract_multiplier.to_decimal_string(), "1");
    }

    #[test]
    fn option_fields_round_trip() {
        let mut inst = InstrumentId::equity("inst-3", "SPY 250117C00500000");
        inst.asset_class = AssetClass::Option;
        inst.option_right = Some(OptionRight::Call);
        inst.exercise_style = Some(ExerciseStyle::American);
        inst.strike = Some(Decimal::parse("500").unwrap());
        inst.contract_multiplier = Decimal::from_i64(100);
        let json = serde_json::to_string(&inst).unwrap();
        let back: InstrumentId = serde_json::from_str(&json).unwrap();
        assert_eq!(back.option_right, Some(OptionRight::Call));
        assert_eq!(back.contract_multiplier.to_decimal_string(), "100");
    }
}

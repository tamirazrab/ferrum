use super::engine::*;
use std::str::FromStr;

#[test]
fn asset_from_str_valid() {
    assert_eq!(Asset::from_str("USDC").unwrap(), Asset::Usdc);
    assert_eq!(Asset::from_str("SOL").unwrap(), Asset::Sol);
    assert_eq!(Asset::from_str("BTC").unwrap(), Asset::Btc);
    assert_eq!(Asset::from_str("ETH").unwrap(), Asset::Eth);
    assert_eq!(Asset::from_str("USDT").unwrap(), Asset::Usdt);
}

#[test]
fn asset_from_str_invalid() {
    assert!(Asset::from_str("DOGE").is_err());
    assert!(Asset::from_str("").is_err());
}

#[test]
fn asset_display() {
    assert_eq!(Asset::Sol.to_string(), "SOL");
    assert_eq!(Asset::Usdc.to_string(), "USDC");
}

#[test]
fn asset_pair_ticker() {
    let pair = AssetPair {
        base: Asset::Sol,
        quote: Asset::Usdc,
    };
    assert_eq!(pair.ticker(), "SOL_USDC");
}

#[test]
fn order_serialization_roundtrip() {
    let order = CreateOrder {
        market: "SOL_USDC".to_string(),
        price: rust_decimal_macros::dec!(100.50),
        quantity: rust_decimal_macros::dec!(5),
        side: OrderSide::Buy,
        user_id: "user1".to_string(),
        pubsub_id: None,
    };

    let json = serde_json::to_string(&order).unwrap();
    let deserialized: CreateOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.market, "SOL_USDC");
    assert_eq!(deserialized.price, rust_decimal_macros::dec!(100.50));
}

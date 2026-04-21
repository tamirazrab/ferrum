use super::engine::Engine;
use super::orderbook::OrderBook;
use crate::types::engine::{
    Asset, AssetPair, CancelAllOrders, CancelOrder, CreateOrder, GetDepth, Order, OrderSide,
    OrderStatus, OrderType,
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn setup_engine() -> Engine {
    let mut engine = Engine::new();
    engine.orderbooks.push(OrderBook::new(
        AssetPair {
            base: Asset::Sol,
            quote: Asset::Usdc,
        },
        0,
    ));
    engine
}

fn create_order(market: &str, side: OrderSide, price: Decimal, qty: Decimal, user: &str) -> CreateOrder {
    CreateOrder {
        market: market.to_string(),
        price,
        quantity: qty,
        side,
        user_id: user.to_string(),
        pubsub_id: None,
    }
}

// --- Fund locking tests ---

#[test]
fn check_and_lock_funds_buy_locks_quote() {
    let mut engine = setup_engine();
    engine.init_user_balance("alice");

    let order = create_order("SOL_USDC", OrderSide::Buy, dec!(100), dec!(10), "alice");
    engine.check_and_lock_funds(&order).unwrap();

    assert_eq!(engine.get_available_balance("alice", &Asset::Usdc).unwrap(), dec!(999000));
    assert_eq!(engine.get_locked_balance("alice", &Asset::Usdc).unwrap(), dec!(1000));
}

#[test]
fn check_and_lock_funds_sell_locks_base() {
    let mut engine = setup_engine();
    engine.init_user_balance("alice");

    let order = create_order("SOL_USDC", OrderSide::Sell, dec!(100), dec!(5), "alice");
    engine.check_and_lock_funds(&order).unwrap();

    assert_eq!(engine.get_available_balance("alice", &Asset::Sol).unwrap(), dec!(9995));
    assert_eq!(engine.get_locked_balance("alice", &Asset::Sol).unwrap(), dec!(5));
}

#[test]
fn insufficient_funds_rejected() {
    let mut engine = setup_engine();
    engine.init_user_balance("alice");

    let order = create_order("SOL_USDC", OrderSide::Buy, dec!(100), dec!(20000), "alice");
    let result = engine.check_and_lock_funds(&order);

    assert!(result.is_err());
}

#[test]
fn unknown_user_rejected() {
    let mut engine = setup_engine();

    let order = create_order("SOL_USDC", OrderSide::Buy, dec!(100), dec!(1), "nobody");
    let result = engine.check_and_lock_funds(&order);

    assert!(result.is_err());
}

// --- Market parsing ---

#[test]
fn parse_market_assets_valid() {
    let (base, quote) = Engine::parse_market_assets("SOL_USDC").unwrap();
    assert_eq!(base, Asset::Sol);
    assert_eq!(quote, Asset::Usdc);
}

#[test]
fn parse_market_assets_invalid() {
    assert!(Engine::parse_market_assets("INVALID").is_err());
    assert!(Engine::parse_market_assets("SOL_UNKNOWN").is_err());
    assert!(Engine::parse_market_assets("").is_err());
}

// --- Cancel with balance unlock ---

#[tokio::test]
async fn cancel_order_unlocks_funds() {
    let mut engine = setup_engine();
    engine.init_user_balance("alice");

    let lock_order = create_order("SOL_USDC", OrderSide::Buy, dec!(100), dec!(10), "alice");
    engine.check_and_lock_funds(&lock_order).unwrap();

    let order_id = {
        let ob = &mut engine.orderbooks[0];
        let order = Order {
            price: dec!(100),
            quantity: dec!(10),
            filled_quantity: dec!(0),
            order_id: "test-order".to_string(),
            user_id: "alice".to_string(),
            side: OrderSide::Buy,
            order_type: OrderType::Limit,
            order_status: OrderStatus::Pending,
            timestamp: 0,
        };
        ob.process_order(order);
        "test-order".to_string()
    };

    let cancel = CancelOrder {
        order_id,
        user_id: "alice".to_string(),
        price: dec!(100),
        side: OrderSide::Buy,
        market: "SOL_USDC".to_string(),
        pubsub_id: None,
    };
    engine.cancel_order(cancel, None).await.unwrap();

    assert_eq!(engine.get_available_balance("alice", &Asset::Usdc).unwrap(), dec!(1000000));
    assert_eq!(engine.get_locked_balance("alice", &Asset::Usdc).unwrap(), dec!(0));
}

// --- Cancel all with balance unlock ---

#[tokio::test]
async fn cancel_all_orders_unlocks_all_funds() {
    let mut engine = setup_engine();
    engine.init_user_balance("alice");

    let buy_order = create_order("SOL_USDC", OrderSide::Buy, dec!(100), dec!(5), "alice");
    engine.check_and_lock_funds(&buy_order).unwrap();
    let ob = &mut engine.orderbooks[0];
    ob.process_order(Order {
        price: dec!(100),
        quantity: dec!(5),
        filled_quantity: dec!(0),
        order_id: "buy1".to_string(),
        user_id: "alice".to_string(),
        side: OrderSide::Buy,
        order_type: OrderType::Limit,
        order_status: OrderStatus::Pending,
        timestamp: 0,
    });

    let sell_order = create_order("SOL_USDC", OrderSide::Sell, dec!(200), dec!(3), "alice");
    engine.check_and_lock_funds(&sell_order).unwrap();
    let ob = &mut engine.orderbooks[0];
    ob.process_order(Order {
        price: dec!(200),
        quantity: dec!(3),
        filled_quantity: dec!(0),
        order_id: "sell1".to_string(),
        user_id: "alice".to_string(),
        side: OrderSide::Sell,
        order_type: OrderType::Limit,
        order_status: OrderStatus::Pending,
        timestamp: 0,
    });

    let cancel_all = CancelAllOrders {
        user_id: "alice".to_string(),
        market: "SOL_USDC".to_string(),
        pubsub_id: None,
    };
    engine.cancel_all_orders(cancel_all, None).await.unwrap();

    assert_eq!(engine.get_available_balance("alice", &Asset::Usdc).unwrap(), dec!(1000000));
    assert_eq!(engine.get_locked_balance("alice", &Asset::Usdc).unwrap(), dec!(0));
    assert_eq!(engine.get_available_balance("alice", &Asset::Sol).unwrap(), dec!(10000));
    assert_eq!(engine.get_locked_balance("alice", &Asset::Sol).unwrap(), dec!(0));
}

// --- Balance updates after a fill ---

#[test]
fn balance_updates_after_buy_fill() {
    let mut engine = setup_engine();
    engine.init_user_balance("buyer");
    engine.init_user_balance("seller");

    let sell_lock = create_order("SOL_USDC", OrderSide::Sell, dec!(100), dec!(5), "seller");
    engine.check_and_lock_funds(&sell_lock).unwrap();

    let ob = &mut engine.orderbooks[0];
    ob.process_order(Order {
        price: dec!(100),
        quantity: dec!(5),
        filled_quantity: dec!(0),
        order_id: "s1".to_string(),
        user_id: "seller".to_string(),
        side: OrderSide::Sell,
        order_type: OrderType::Limit,
        order_status: OrderStatus::Pending,
        timestamp: 0,
    });

    let buy_lock = create_order("SOL_USDC", OrderSide::Buy, dec!(100), dec!(5), "buyer");
    engine.check_and_lock_funds(&buy_lock).unwrap();

    let order = Order {
        price: dec!(100),
        quantity: dec!(5),
        filled_quantity: dec!(0),
        order_id: "b1".to_string(),
        user_id: "buyer".to_string(),
        side: OrderSide::Buy,
        order_type: OrderType::Limit,
        order_status: OrderStatus::Pending,
        timestamp: 0,
    };
    let ob = &mut engine.orderbooks[0];
    let result = ob.process_order(order.clone());

    engine
        .update_user_balance(Asset::Sol, Asset::Usdc, &order, &result)
        .unwrap();

    assert_eq!(engine.get_available_balance("buyer", &Asset::Sol).unwrap(), dec!(10005));
    assert_eq!(engine.get_locked_balance("buyer", &Asset::Usdc).unwrap(), dec!(0));

    assert_eq!(engine.get_available_balance("seller", &Asset::Usdc).unwrap(), dec!(1000500));
    assert_eq!(engine.get_locked_balance("seller", &Asset::Sol).unwrap(), dec!(0));
}

// --- Depth from engine ---

#[test]
fn get_depth_returns_correct_data() {
    let mut engine = setup_engine();

    let ob = &mut engine.orderbooks[0];
    ob.process_order(Order {
        price: dec!(100),
        quantity: dec!(10),
        filled_quantity: dec!(0),
        order_id: "b1".to_string(),
        user_id: "user1".to_string(),
        side: OrderSide::Buy,
        order_type: OrderType::Limit,
        order_status: OrderStatus::Pending,
        timestamp: 0,
    });

    let (bids, asks) = engine.get_depth(GetDepth {
        symbol: "SOL_USDC".to_string(),
        pubsub_id: None,
    });

    assert_eq!(bids.len(), 1);
    assert_eq!(bids[0], (dec!(100), dec!(10)));
    assert!(asks.is_empty());
}

#[test]
fn get_depth_nonexistent_market_returns_empty() {
    let engine = setup_engine();
    let (bids, asks) = engine.get_depth(GetDepth {
        symbol: "FAKE_MKT".to_string(),
        pubsub_id: None,
    });
    assert!(bids.is_empty());
    assert!(asks.is_empty());
}

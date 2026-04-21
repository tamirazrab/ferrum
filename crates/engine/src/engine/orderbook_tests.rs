use super::orderbook::OrderBook;
use crate::types::engine::{
    Asset, AssetPair, CancelOrder, Order, OrderSide, OrderStatus, OrderType,
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn test_pair() -> AssetPair {
    AssetPair {
        base: Asset::Sol,
        quote: Asset::Usdc,
    }
}

fn make_order(
    side: OrderSide,
    price: Decimal,
    quantity: Decimal,
    user_id: &str,
    order_id: &str,
) -> Order {
    Order {
        price,
        quantity,
        filled_quantity: dec!(0),
        order_id: order_id.to_string(),
        user_id: user_id.to_string(),
        side,
        order_type: OrderType::Limit,
        order_status: OrderStatus::Pending,
        timestamp: 0,
    }
}

fn make_cancel(order_id: &str, price: Decimal, side: OrderSide) -> CancelOrder {
    CancelOrder {
        order_id: order_id.to_string(),
        user_id: "user1".to_string(),
        price,
        side,
        market: "SOL_USDC".to_string(),
        pubsub_id: None,
    }
}

// --- Empty book tests ---

#[test]
fn buy_into_empty_book_rests() {
    let mut book = OrderBook::new(test_pair(), 0);
    let order = make_order(OrderSide::Buy, dec!(100), dec!(10), "buyer", "b1");
    let result = book.process_order(order);

    assert!(result.fills.is_empty());
    assert_eq!(result.executed_quantity, dec!(0));
    assert_eq!(book.bids.len(), 1);
    assert_eq!(book.asks.len(), 0);
}

#[test]
fn sell_into_empty_book_rests() {
    let mut book = OrderBook::new(test_pair(), 0);
    let order = make_order(OrderSide::Sell, dec!(100), dec!(5), "seller", "s1");
    let result = book.process_order(order);

    assert!(result.fills.is_empty());
    assert_eq!(result.executed_quantity, dec!(0));
    assert_eq!(book.asks.len(), 1);
    assert_eq!(book.bids.len(), 0);
}

// --- Exact match tests ---

#[test]
fn exact_fill_buy_against_ask() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Sell, dec!(100), dec!(5), "seller", "s1"));

    let result = book.process_order(make_order(
        OrderSide::Buy, dec!(100), dec!(5), "buyer", "b1",
    ));

    assert_eq!(result.fills.len(), 1);
    assert_eq!(result.executed_quantity, dec!(5));
    assert_eq!(result.fills[0].price, dec!(100));
    assert_eq!(result.fills[0].quantity, dec!(5));
    assert!(book.asks.is_empty());
    assert!(book.bids.is_empty());
}

#[test]
fn exact_fill_sell_against_bid() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Buy, dec!(100), dec!(5), "buyer", "b1"));

    let result = book.process_order(make_order(
        OrderSide::Sell, dec!(100), dec!(5), "seller", "s1",
    ));

    assert_eq!(result.fills.len(), 1);
    assert_eq!(result.executed_quantity, dec!(5));
    assert!(book.bids.is_empty());
    assert!(book.asks.is_empty());
}

// --- Partial fill tests ---

#[test]
fn partial_fill_taker_smaller_than_maker() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Sell, dec!(100), dec!(10), "seller", "s1"));

    let result = book.process_order(make_order(
        OrderSide::Buy, dec!(100), dec!(3), "buyer", "b1",
    ));

    assert_eq!(result.executed_quantity, dec!(3));
    assert_eq!(result.fills.len(), 1);
    assert_eq!(result.fills[0].quantity, dec!(3));

    assert!(book.bids.is_empty());
    let remaining_ask = book.asks.get(&dec!(100)).unwrap();
    assert_eq!(remaining_ask[0].filled_quantity, dec!(3));
}

#[test]
fn partial_fill_taker_larger_than_maker_rests_remainder() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Sell, dec!(100), dec!(3), "seller", "s1"));

    let result = book.process_order(make_order(
        OrderSide::Buy, dec!(100), dec!(10), "buyer", "b1",
    ));

    assert_eq!(result.executed_quantity, dec!(3));
    assert!(book.asks.is_empty());
    let resting_bid = book.bids.get(&dec!(100)).unwrap();
    assert_eq!(resting_bid[0].quantity, dec!(10));
    assert_eq!(resting_bid[0].filled_quantity, dec!(3));
}

// --- Multi-level matching ---

#[test]
fn buy_sweeps_multiple_ask_levels() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Sell, dec!(100), dec!(3), "s1", "ask1"));
    book.process_order(make_order(OrderSide::Sell, dec!(101), dec!(4), "s2", "ask2"));
    book.process_order(make_order(OrderSide::Sell, dec!(102), dec!(5), "s3", "ask3"));

    let result = book.process_order(make_order(
        OrderSide::Buy, dec!(102), dec!(10), "buyer", "b1",
    ));

    assert_eq!(result.executed_quantity, dec!(10));
    assert_eq!(result.fills.len(), 3);
    assert_eq!(result.fills[0].quantity, dec!(3)); // $100 level
    assert_eq!(result.fills[1].quantity, dec!(4)); // $101 level
    assert_eq!(result.fills[2].quantity, dec!(3)); // $102 level, partial

    assert!(book.bids.is_empty());
    let remaining_ask_102 = book.asks.get(&dec!(102)).unwrap();
    assert_eq!(remaining_ask_102[0].filled_quantity, dec!(3));
}

#[test]
fn sell_sweeps_multiple_bid_levels() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Buy, dec!(102), dec!(3), "b1", "bid1"));
    book.process_order(make_order(OrderSide::Buy, dec!(101), dec!(4), "b2", "bid2"));
    book.process_order(make_order(OrderSide::Buy, dec!(100), dec!(5), "b3", "bid3"));

    let result = book.process_order(make_order(
        OrderSide::Sell, dec!(100), dec!(10), "seller", "s1",
    ));

    assert_eq!(result.executed_quantity, dec!(10));
    assert_eq!(result.fills.len(), 3);
    assert_eq!(result.fills[0].quantity, dec!(3)); // $102 (best bid)
    assert_eq!(result.fills[1].quantity, dec!(4)); // $101
    assert_eq!(result.fills[2].quantity, dec!(3)); // $100, partial
}

// --- Price priority ---

#[test]
fn buy_fills_at_best_ask_price_not_taker_price() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Sell, dec!(95), dec!(5), "seller", "s1"));

    let result = book.process_order(make_order(
        OrderSide::Buy, dec!(100), dec!(5), "buyer", "b1",
    ));

    assert_eq!(result.fills[0].price, dec!(95)); // maker price
}

#[test]
fn no_match_if_buy_price_below_ask() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Sell, dec!(100), dec!(5), "seller", "s1"));

    let result = book.process_order(make_order(
        OrderSide::Buy, dec!(99), dec!(5), "buyer", "b1",
    ));

    assert!(result.fills.is_empty());
    assert_eq!(book.bids.len(), 1);
    assert_eq!(book.asks.len(), 1);
}

#[test]
fn no_match_if_sell_price_above_bid() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Buy, dec!(100), dec!(5), "buyer", "b1"));

    let result = book.process_order(make_order(
        OrderSide::Sell, dec!(101), dec!(5), "seller", "s1",
    ));

    assert!(result.fills.is_empty());
    assert_eq!(book.bids.len(), 1);
    assert_eq!(book.asks.len(), 1);
}

// --- FIFO within a price level ---

#[test]
fn fifo_priority_at_same_price_level() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Sell, dec!(100), dec!(3), "first", "s1"));
    book.process_order(make_order(OrderSide::Sell, dec!(100), dec!(3), "second", "s2"));

    let result = book.process_order(make_order(
        OrderSide::Buy, dec!(100), dec!(4), "buyer", "b1",
    ));

    assert_eq!(result.fills.len(), 2);
    assert_eq!(result.fills[0].other_user_id, "first");
    assert_eq!(result.fills[0].quantity, dec!(3));
    assert_eq!(result.fills[1].other_user_id, "second");
    assert_eq!(result.fills[1].quantity, dec!(1));
}

// --- Cancel order tests ---

#[test]
fn cancel_existing_order() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Buy, dec!(100), dec!(5), "user1", "b1"));

    let cancel = make_cancel("b1", dec!(100), OrderSide::Buy);
    let result = book.cancel_order(&cancel);

    assert!(result.is_some());
    assert_eq!(result.unwrap().order_id, "b1");
    assert!(book.bids.is_empty());
}

#[test]
fn cancel_nonexistent_order_returns_none() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Buy, dec!(100), dec!(5), "user1", "b1"));

    let cancel = make_cancel("nonexistent", dec!(100), OrderSide::Buy);
    assert!(book.cancel_order(&cancel).is_none());
}

#[test]
fn cancel_wrong_price_returns_none() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Buy, dec!(100), dec!(5), "user1", "b1"));

    let cancel = make_cancel("b1", dec!(99), OrderSide::Buy);
    assert!(book.cancel_order(&cancel).is_none());
}

// --- Cancel all orders ---

#[test]
fn cancel_all_returns_all_user_orders() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Buy, dec!(100), dec!(5), "user1", "b1"));
    book.process_order(make_order(OrderSide::Sell, dec!(110), dec!(3), "user1", "s1"));
    book.process_order(make_order(OrderSide::Buy, dec!(99), dec!(2), "user2", "b2"));

    let cancelled = book.cancel_all_orders("user1");

    assert_eq!(cancelled.len(), 2);
    assert!(cancelled.iter().any(|o| o.order_id == "b1"));
    assert!(cancelled.iter().any(|o| o.order_id == "s1"));

    assert!(book.get_open_orders("user1").is_empty());
    assert_eq!(book.get_open_orders("user2").len(), 1);
}

#[test]
fn cancel_all_for_user_with_no_orders_returns_empty() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Buy, dec!(100), dec!(5), "user1", "b1"));

    let cancelled = book.cancel_all_orders("nobody");
    assert!(cancelled.is_empty());
    assert_eq!(book.get_open_orders("user1").len(), 1);
}

// --- Depth tests ---

#[test]
fn depth_reflects_remaining_quantity_not_total() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Sell, dec!(100), dec!(10), "seller", "s1"));
    book.process_order(make_order(OrderSide::Buy, dec!(100), dec!(3), "buyer", "b1"));

    let (bids, asks) = book.get_depth();
    assert!(bids.is_empty());
    assert_eq!(asks.len(), 1);
    assert_eq!(asks[0], (dec!(100), dec!(7)));
}

#[test]
fn depth_empty_book() {
    let book = OrderBook::new(test_pair(), 0);
    let (bids, asks) = book.get_depth();
    assert!(bids.is_empty());
    assert!(asks.is_empty());
}

#[test]
fn depth_aggregates_multiple_orders_at_same_price() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Buy, dec!(100), dec!(5), "u1", "b1"));
    book.process_order(make_order(OrderSide::Buy, dec!(100), dec!(3), "u2", "b2"));

    let (bids, _) = book.get_depth();
    assert_eq!(bids.len(), 1);
    assert_eq!(bids[0], (dec!(100), dec!(8)));
}

// --- Open orders tests ---

#[test]
fn get_open_order_found() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Buy, dec!(100), dec!(5), "user1", "b1"));

    let order = book.get_open_order("user1", "b1");
    assert!(order.is_some());
    assert_eq!(order.unwrap().order_id, "b1");
}

#[test]
fn get_open_order_not_found() {
    let book = OrderBook::new(test_pair(), 0);
    assert!(book.get_open_order("user1", "b1").is_none());
}

#[test]
fn get_open_orders_returns_all_for_user() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Buy, dec!(100), dec!(5), "user1", "b1"));
    book.process_order(make_order(OrderSide::Sell, dec!(110), dec!(3), "user1", "s1"));
    book.process_order(make_order(OrderSide::Buy, dec!(99), dec!(2), "user2", "b2"));

    let orders = book.get_open_orders("user1");
    assert_eq!(orders.len(), 2);
}

// --- Trade ID increments ---

#[test]
fn trade_ids_increment_correctly() {
    let mut book = OrderBook::new(test_pair(), 100);
    book.process_order(make_order(OrderSide::Sell, dec!(100), dec!(3), "s1", "ask1"));
    book.process_order(make_order(OrderSide::Sell, dec!(101), dec!(4), "s2", "ask2"));

    let result = book.process_order(make_order(
        OrderSide::Buy, dec!(101), dec!(7), "buyer", "b1",
    ));

    assert_eq!(result.fills[0].trade_id, 101);
    assert_eq!(result.fills[1].trade_id, 102);
}

// --- Empty level cleanup ---

#[test]
fn empty_price_levels_removed_after_full_fill() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Sell, dec!(100), dec!(5), "seller", "s1"));

    book.process_order(make_order(
        OrderSide::Buy, dec!(100), dec!(5), "buyer", "b1",
    ));

    assert!(book.asks.is_empty());
    assert!(!book.asks.contains_key(&dec!(100)));
}

// --- Self-trade scenario ---

#[test]
fn self_trade_allowed_by_default() {
    let mut book = OrderBook::new(test_pair(), 0);
    book.process_order(make_order(OrderSide::Sell, dec!(100), dec!(5), "user1", "s1"));

    let result = book.process_order(make_order(
        OrderSide::Buy, dec!(100), dec!(5), "user1", "b1",
    ));

    assert_eq!(result.fills.len(), 1);
    assert_eq!(result.fills[0].other_user_id, "user1");
}

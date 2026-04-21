use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::types::engine::{
    AssetPair, CancelOrder, Fill, Order, OrderSide, OrderStatus, ProcessOrderResult,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBook {
    pub bids: BTreeMap<Decimal, Vec<Order>>,
    pub asks: BTreeMap<Decimal, Vec<Order>>,
    pub asset_pair: AssetPair,
    pub trade_id: i64,
    last_update_id: i64,
}

impl OrderBook {
    pub fn new(asset_pair: AssetPair, trade_id: i64) -> OrderBook {
        OrderBook {
            asks: BTreeMap::new(),
            bids: BTreeMap::new(),
            asset_pair,
            trade_id,
            last_update_id: 0,
        }
    }

    pub fn ticker(&self) -> String {
        self.asset_pair.ticker()
    }

    pub fn process_order(&mut self, mut order: Order) -> ProcessOrderResult {
        let result = match order.side {
            OrderSide::Buy => self.match_asks(&order),
            OrderSide::Sell => self.match_bids(&order),
        };

        order.filled_quantity = result.executed_quantity;

        if result.executed_quantity >= order.quantity {
            order.order_status = OrderStatus::Filled;
        } else if result.executed_quantity > Decimal::ZERO {
            order.order_status = OrderStatus::PartiallyFilled;
        }

        if result.executed_quantity < order.quantity {
            let side_map = match order.side {
                OrderSide::Buy => &mut self.bids,
                OrderSide::Sell => &mut self.asks,
            };
            side_map
                .entry(order.price)
                .or_default()
                .push(order);
        }

        result
    }

    fn match_asks(&mut self, order: &Order) -> ProcessOrderResult {
        let mut fills: Vec<Fill> = vec![];
        let mut executed_quantity: Decimal = dec!(0);

        for (_price, asks) in self.asks.iter_mut() {
            for ask in asks.iter_mut() {
                if order.price >= ask.price && executed_quantity < order.quantity {
                    let taker_remaining = order.quantity - executed_quantity;
                    let maker_remaining = ask.quantity - ask.filled_quantity;
                    let filled_quantity = std::cmp::min(taker_remaining, maker_remaining);

                    self.trade_id += 1;
                    executed_quantity += filled_quantity;
                    ask.filled_quantity += filled_quantity;

                    fills.push(Fill {
                        price: ask.price,
                        quantity: filled_quantity,
                        trade_id: self.trade_id,
                        other_user_id: ask.user_id.clone(),
                        order_id: ask.order_id.clone(),
                    });
                }
            }

            asks.retain(|ask| ask.filled_quantity < ask.quantity);
        }

        self.asks.retain(|_, orders| !orders.is_empty());

        ProcessOrderResult {
            fills,
            executed_quantity,
        }
    }

    fn match_bids(&mut self, order: &Order) -> ProcessOrderResult {
        let mut fills: Vec<Fill> = vec![];
        let mut executed_quantity: Decimal = dec!(0);

        for (_price, bids) in self.bids.iter_mut().rev() {
            for bid in bids.iter_mut() {
                if order.price <= bid.price && executed_quantity < order.quantity {
                    let taker_remaining = order.quantity - executed_quantity;
                    let maker_remaining = bid.quantity - bid.filled_quantity;
                    let filled_quantity = std::cmp::min(taker_remaining, maker_remaining);

                    self.trade_id += 1;
                    executed_quantity += filled_quantity;
                    bid.filled_quantity += filled_quantity;

                    fills.push(Fill {
                        price: bid.price,
                        quantity: filled_quantity,
                        trade_id: self.trade_id,
                        other_user_id: bid.user_id.clone(),
                        order_id: bid.order_id.clone(),
                    });
                }
            }

            bids.retain(|bid| bid.filled_quantity < bid.quantity);
        }

        self.bids.retain(|_, orders| !orders.is_empty());

        ProcessOrderResult {
            fills,
            executed_quantity,
        }
    }

    pub fn get_open_order(&self, user_id: &str, order_id: &str) -> Option<&Order> {
        self.bids
            .values()
            .chain(self.asks.values())
            .flat_map(|orders| orders.iter())
            .find(|order| order.user_id == user_id && order.order_id == order_id)
    }

    pub fn get_open_orders(&self, user_id: &str) -> Vec<&Order> {
        self.bids
            .values()
            .chain(self.asks.values())
            .flat_map(|orders| orders.iter())
            .filter(|order| order.user_id == user_id)
            .collect()
    }

    pub fn cancel_order(&mut self, cancel_order: &CancelOrder) -> Option<Order> {
        let side_map = match cancel_order.side {
            OrderSide::Buy => &mut self.bids,
            OrderSide::Sell => &mut self.asks,
        };

        let orders = side_map.get_mut(&cancel_order.price)?;
        let index = orders
            .iter()
            .position(|order| order.order_id == cancel_order.order_id)?;
        let order = orders.remove(index);

        if orders.is_empty() {
            side_map.remove(&cancel_order.price);
        }

        Some(order)
    }

    /// Removes all orders for `user_id` and returns the cancelled orders
    /// so the caller can unlock the associated funds.
    pub fn cancel_all_orders(&mut self, user_id: &str) -> Vec<Order> {
        let mut cancelled = Vec::new();

        for orders in self.bids.values_mut() {
            let (user_orders, remaining): (Vec<_>, Vec<_>) =
                orders.drain(..).partition(|o| o.user_id == user_id);
            cancelled.extend(user_orders);
            *orders = remaining;
        }

        for orders in self.asks.values_mut() {
            let (user_orders, remaining): (Vec<_>, Vec<_>) =
                orders.drain(..).partition(|o| o.user_id == user_id);
            cancelled.extend(user_orders);
            *orders = remaining;
        }

        self.bids.retain(|_, orders| !orders.is_empty());
        self.asks.retain(|_, orders| !orders.is_empty());

        cancelled
    }

    pub fn get_depth(&self) -> (Vec<(Decimal, Decimal)>, Vec<(Decimal, Decimal)>) {
        let aggregate = |side: &BTreeMap<Decimal, Vec<Order>>| -> Vec<(Decimal, Decimal)> {
            side.iter()
                .map(|(price, orders)| {
                    let remaining: Decimal = orders
                        .iter()
                        .map(|o| o.quantity - o.filled_quantity)
                        .sum();
                    (*price, remaining)
                })
                .filter(|(_, qty)| *qty > Decimal::ZERO)
                .collect()
        };

        (aggregate(&self.bids), aggregate(&self.asks))
    }
}

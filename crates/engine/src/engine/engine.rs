use crate::engine::db::DbUpdates;
use crate::engine::error::EngineError;
use crate::engine::orderbook::OrderBook;
use crate::engine::ws_stream::WsStreamUpdates;
use crate::types::engine::{
    Asset, AssetPair, CancelAllOrders, CancelOrder, CreateOrder, GetDepth, GetOpenOrder,
    GetOpenOrders, Order, OrderSide, OrderStatus, OrderType, ProcessOrderResult,
};
use db_processor::query::{get_latest_trade_id_from_db, load_open_orders, load_user_balances};
use redis::RedisManager;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Postgres};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Mutex;

pub enum AmountType {
    Available,
    Locked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Amount {
    available: Decimal,
    locked: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserBalances {
    user_id: String,
    balance: HashMap<Asset, Amount>,
}

/// Core exchange engine holding all order books and user balances in memory.
#[derive(Debug, Serialize, Deserialize)]
pub struct Engine {
    pub orderbooks: Vec<OrderBook>,
    pub balances: HashMap<String, Mutex<UserBalances>>,
}

impl Engine {
    pub fn new() -> Engine {
        Engine {
            orderbooks: vec![],
            balances: HashMap::new(),
        }
    }

    pub async fn init_engine(&mut self, pool: &Pool<Postgres>) -> Result<(), EngineError> {
        let market = "SOL_USDC".to_string();
        let trade_id = get_latest_trade_id_from_db(pool, market)
            .await
            .map_err(|e| EngineError::RedisError(e.to_string()))?;

        let orderbook = OrderBook::new(
            AssetPair {
                base: Asset::Sol,
                quote: Asset::Usdc,
            },
            trade_id + 1,
        );

        self.orderbooks.push(orderbook);

        self.restore_from_db(pool).await?;

        Ok(())
    }

    /// Reload persisted open orders and user balances from Postgres.
    /// If the tables are empty this is a no-op (fresh start).
    pub async fn restore_from_db(&mut self, pool: &Pool<Postgres>) -> Result<(), EngineError> {
        let saved_orders = load_open_orders(pool)
            .await
            .map_err(|e| EngineError::RedisError(e.to_string()))?;

        for row in saved_orders {
            let side = match row.side.as_str() {
                "Buy" => OrderSide::Buy,
                "Sell" => OrderSide::Sell,
                _ => continue,
            };
            let status = match row.order_status.as_str() {
                "Pending" => OrderStatus::Pending,
                "PartiallyFilled" => OrderStatus::PartiallyFilled,
                _ => continue,
            };

            let order = Order {
                order_id: row.order_id,
                user_id: row.user_id,
                price: row.price,
                quantity: row.quantity,
                filled_quantity: row.filled_quantity,
                side,
                order_type: OrderType::Limit,
                order_status: status,
                timestamp: row.timestamp,
            };

            if let Some(ob) = self
                .orderbooks
                .iter_mut()
                .find(|ob| ob.ticker() == row.market)
            {
                let side_map = match order.side {
                    OrderSide::Buy => &mut ob.bids,
                    OrderSide::Sell => &mut ob.asks,
                };
                side_map.entry(order.price).or_default().push(order);
            }
        }

        let saved_balances = load_user_balances(pool)
            .await
            .map_err(|e| EngineError::RedisError(e.to_string()))?;

        let mut grouped: HashMap<String, HashMap<String, (Decimal, Decimal)>> = HashMap::new();
        for row in saved_balances {
            grouped
                .entry(row.user_id.clone())
                .or_default()
                .insert(row.asset, (row.available, row.locked));
        }

        for (user_id, assets) in grouped {
            let mut balance_map = HashMap::new();
            for (asset_str, (available, locked)) in assets {
                if let Ok(asset) = Asset::from_str(&asset_str) {
                    balance_map.insert(asset, Amount { available, locked });
                }
            }
            if !balance_map.is_empty() {
                self.balances.insert(
                    user_id.clone(),
                    Mutex::new(UserBalances {
                        user_id,
                        balance: balance_map,
                    }),
                );
            }
        }

        Ok(())
    }

    pub fn init_user_balance(&mut self, user_id: &str) {
        let mut balances_map = HashMap::new();

        balances_map.insert(
            Asset::Usdc,
            Amount {
                available: Decimal::new(1_000_000, 0),
                locked: Decimal::ZERO,
            },
        );

        balances_map.insert(
            Asset::Sol,
            Amount {
                available: Decimal::new(10_000, 0),
                locked: Decimal::ZERO,
            },
        );

        self.balances.insert(
            user_id.to_string(),
            Mutex::new(UserBalances {
                user_id: user_id.to_string(),
                balance: balances_map,
            }),
        );
    }

    fn find_orderbook(&mut self, market: &str) -> Result<&mut OrderBook, EngineError> {
        self.orderbooks
            .iter_mut()
            .find(|ob| ob.ticker() == market)
            .ok_or_else(|| EngineError::OrderBookNotFound(market.to_string()))
    }

    pub(crate) fn parse_market_assets(market: &str) -> Result<(Asset, Asset), EngineError> {
        let parts: Vec<&str> = market.split('_').collect();
        if parts.len() != 2 {
            return Err(EngineError::InvalidMarketFormat(market.to_string()));
        }
        let base = Asset::from_str(parts[0])
            .map_err(|_| EngineError::UnsupportedAsset(parts[0].to_string()))?;
        let quote = Asset::from_str(parts[1])
            .map_err(|_| EngineError::UnsupportedAsset(parts[1].to_string()))?;
        Ok((base, quote))
    }

    #[tracing::instrument(
        skip(self, redis_conn),
        fields(market = %input_order.market, user_id = %input_order.user_id)
    )]
    pub async fn create_order(
        &mut self,
        input_order: CreateOrder,
        redis_conn: &RedisManager,
    ) -> Result<String, EngineError> {
        if input_order.price <= Decimal::ZERO {
            return Err(EngineError::InvalidPrice);
        }
        if input_order.quantity <= Decimal::ZERO {
            return Err(EngineError::InvalidQuantity);
        }

        self.check_and_lock_funds(&input_order)?;

        let (base_asset, quote_asset) = Self::parse_market_assets(&input_order.market)?;
        let order_id = uuid::Uuid::new_v4().to_string();

        let order = Order {
            price: input_order.price,
            quantity: input_order.quantity,
            filled_quantity: dec!(0),
            order_id: order_id.clone(),
            user_id: input_order.user_id.clone(),
            side: input_order.side,
            order_type: OrderType::Limit,
            order_status: OrderStatus::Pending,
            timestamp: chrono::Utc::now().timestamp_millis(),
        };

        let orderbook = self.find_orderbook(&input_order.market)?;
        let order_result: ProcessOrderResult = orderbook.process_order(order.clone());

        self.update_user_balance(
            base_asset,
            quote_asset,
            &order,
            &order_result,
        )?;

        let _ = self
            .update_db_orders(
                order.clone(),
                input_order.market.as_str(),
                order_result.executed_quantity,
                &order_result.fills,
                redis_conn,
            )
            .await;

        let _ = self
            .create_db_trades(
                input_order.user_id.clone(),
                input_order.market.clone(),
                &order_result.fills,
                redis_conn,
            )
            .await;

        let mut balance_users: std::collections::HashSet<String> =
            std::iter::once(input_order.user_id.clone()).collect();
        for f in &order_result.fills {
            balance_users.insert(f.other_user_id.clone());
        }
        for uid in balance_users {
            self.sync_user_balances_to_db(&uid, redis_conn).await;
        }

        let _ = self
            .publish_ws_trades(
                input_order.market.clone(),
                input_order.user_id.clone(),
                &order_result.fills,
                order.timestamp,
                redis_conn,
            )
            .await;

        let _ = self
            .publish_ws_depth_updates(
                input_order.market.clone(),
                order.price,
                order.side,
                &order_result.fills,
                redis_conn,
            )
            .await;

        Ok(order_id)
    }

    pub fn get_open_order(&self, open_order: GetOpenOrder) -> Result<Order, EngineError> {
        let orderbook = self
            .orderbooks
            .iter()
            .find(|ob| ob.ticker() == open_order.market)
            .ok_or_else(|| EngineError::OrderBookNotFound(open_order.market.clone()))?;

        orderbook
            .get_open_order(&open_order.user_id, &open_order.order_id)
            .cloned()
            .ok_or(EngineError::OrderNotFound)
    }

    #[tracing::instrument(
        skip(self, redis_conn),
        fields(market = %cancel_order.market, user_id = %cancel_order.user_id)
    )]
    pub async fn cancel_order(
        &mut self,
        cancel_order: CancelOrder,
        redis_conn: Option<&RedisManager>,
    ) -> Result<String, EngineError> {
        let (base_asset, quote_asset) = Self::parse_market_assets(&cancel_order.market)?;
        let cancel_order_id = cancel_order.order_id.clone();

        let orderbook = self.find_orderbook(&cancel_order.market)?;
        let order = orderbook
            .cancel_order(&cancel_order)
            .ok_or(EngineError::OrderNotFound)?;

        let user_id = order.user_id.clone();
        let remaining = order.quantity - order.filled_quantity;
        let (asset, amount) = match order.side {
            OrderSide::Buy => (quote_asset, remaining * order.price),
            OrderSide::Sell => (base_asset, remaining),
        };

        self.update_balance_with_lock(
            &order.user_id,
            &asset,
            amount,
            AmountType::Available,
        )?;
        self.update_balance_with_lock(
            &order.user_id,
            &asset,
            -amount,
            AmountType::Locked,
        )?;

        if let Some(rc) = redis_conn {
            DbUpdates::delete_order_from_db(self, &cancel_order_id, rc).await;
            self.sync_user_balances_to_db(&user_id, rc).await;
        }

        Ok(cancel_order_id)
    }

    /// Persist all asset balances for a user to Postgres via the database queue.
    pub async fn sync_user_balances_to_db(&self, user_id: &str, redis_conn: &RedisManager) {
        let snapshots: Vec<(String, Decimal, Decimal)> = {
            let Some(mutex) = self.balances.get(user_id) else {
                return;
            };
            let Ok(guard) = mutex.lock() else {
                return;
            };
            guard
                .balance
                .iter()
                .map(|(asset, amount)| {
                    (asset.to_string(), amount.available, amount.locked)
                })
                .collect()
        };
        for (asset_str, available, locked) in snapshots {
            DbUpdates::persist_balance_to_db(
                self,
                user_id,
                &asset_str,
                available,
                locked,
                redis_conn,
            )
            .await;
        }
    }

    pub fn get_open_orders(&self, open_orders: GetOpenOrders) -> Vec<Order> {
        let orderbook = match self
            .orderbooks
            .iter()
            .find(|ob| ob.ticker() == open_orders.market)
        {
            Some(ob) => ob,
            None => return Vec::new(),
        };

        orderbook
            .get_open_orders(&open_orders.user_id)
            .into_iter()
            .cloned()
            .collect()
    }

    pub async fn cancel_all_orders(
        &mut self,
        cancel_all_orders: CancelAllOrders,
        redis_conn: Option<&RedisManager>,
    ) -> Result<String, EngineError> {
        let (base_asset, quote_asset) = Self::parse_market_assets(&cancel_all_orders.market)?;

        let orderbook = self.find_orderbook(&cancel_all_orders.market)?;
        let cancelled_orders = orderbook.cancel_all_orders(&cancel_all_orders.user_id);
        let cancelled_ids: Vec<String> = cancelled_orders
            .iter()
            .map(|o| o.order_id.clone())
            .collect();

        let balance_updates: Vec<(String, Asset, Decimal)> = cancelled_orders
            .iter()
            .map(|order| {
                let remaining = order.quantity - order.filled_quantity;
                match order.side {
                    OrderSide::Buy => {
                        (order.user_id.clone(), quote_asset.clone(), remaining * order.price)
                    }
                    OrderSide::Sell => {
                        (order.user_id.clone(), base_asset.clone(), remaining)
                    }
                }
            })
            .collect();

        for (user_id, asset, amount) in balance_updates {
            self.update_balance_with_lock(&user_id, &asset, amount, AmountType::Available)?;
            self.update_balance_with_lock(&user_id, &asset, -amount, AmountType::Locked)?;
        }

        if let Some(rc) = redis_conn {
            for oid in cancelled_ids {
                DbUpdates::delete_order_from_db(self, &oid, rc).await;
            }
            self.sync_user_balances_to_db(&cancel_all_orders.user_id, rc)
                .await;
        }

        Ok(format!(
            "All orders for user {} cancelled successfully",
            cancel_all_orders.user_id
        ))
    }

    pub fn get_depth(
        &self,
        depth: GetDepth,
    ) -> (Vec<(Decimal, Decimal)>, Vec<(Decimal, Decimal)>) {
        let orderbook = match self
            .orderbooks
            .iter()
            .find(|ob| ob.ticker() == depth.symbol)
        {
            Some(ob) => ob,
            None => return (Vec::new(), Vec::new()),
        };

        orderbook.get_depth()
    }

    pub fn check_and_lock_funds(&mut self, order: &CreateOrder) -> Result<(), EngineError> {
        let (base_asset, quote_asset) = Self::parse_market_assets(&order.market)?;
        let user_id = &order.user_id;

        let user_balance_mutex = self
            .balances
            .get_mut(user_id)
            .ok_or_else(|| EngineError::UserNotFound(user_id.clone()))?;

        let mut user_balance = user_balance_mutex
            .lock()
            .map_err(|_| EngineError::MutexPoisoned)?;

        match order.side {
            OrderSide::Buy => {
                let balance = user_balance
                    .balance
                    .get_mut(&quote_asset)
                    .ok_or_else(|| EngineError::NoBalanceForAsset(quote_asset.to_string()))?;

                let total_cost = order.price * order.quantity;
                if balance.available >= total_cost {
                    balance.available -= total_cost;
                    balance.locked += total_cost;
                } else {
                    return Err(EngineError::InsufficientFunds);
                }
            }
            OrderSide::Sell => {
                let balance = user_balance
                    .balance
                    .get_mut(&base_asset)
                    .ok_or_else(|| EngineError::NoBalanceForAsset(base_asset.to_string()))?;

                if balance.available >= order.quantity {
                    balance.available -= order.quantity;
                    balance.locked += order.quantity;
                } else {
                    return Err(EngineError::InsufficientFunds);
                }
            }
        }

        Ok(())
    }

    pub fn update_user_balance(
        &mut self,
        base_asset: Asset,
        quote_asset: Asset,
        order: &Order,
        order_result: &ProcessOrderResult,
    ) -> Result<(), EngineError> {
        for fill in &order_result.fills {
            let quote_amount = fill.price * fill.quantity;

            match order.side {
                OrderSide::Buy => {
                    self.update_balance_with_lock(
                        &order.user_id, &base_asset, fill.quantity, AmountType::Available,
                    )?;
                    self.update_balance_with_lock(
                        &order.user_id, &quote_asset, -quote_amount, AmountType::Locked,
                    )?;
                    self.update_balance_with_lock(
                        &fill.other_user_id, &quote_asset, quote_amount, AmountType::Available,
                    )?;
                    self.update_balance_with_lock(
                        &fill.other_user_id, &base_asset, -fill.quantity, AmountType::Locked,
                    )?;
                }
                OrderSide::Sell => {
                    self.update_balance_with_lock(
                        &order.user_id, &base_asset, -fill.quantity, AmountType::Locked,
                    )?;
                    self.update_balance_with_lock(
                        &order.user_id, &quote_asset, quote_amount, AmountType::Available,
                    )?;
                    self.update_balance_with_lock(
                        &fill.other_user_id, &base_asset, fill.quantity, AmountType::Available,
                    )?;
                    self.update_balance_with_lock(
                        &fill.other_user_id, &quote_asset, -quote_amount, AmountType::Locked,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn update_balance_with_lock(
        &self,
        user_id: &str,
        asset: &Asset,
        amount: Decimal,
        amount_type: AmountType,
    ) -> Result<(), EngineError> {
        let user_balance_mutex = self
            .balances
            .get(user_id)
            .ok_or_else(|| EngineError::UserNotFound(user_id.to_string()))?;

        let mut user_balance = user_balance_mutex
            .lock()
            .map_err(|_| EngineError::MutexPoisoned)?;

        let balance = user_balance
            .balance
            .get_mut(asset)
            .ok_or_else(|| EngineError::NoBalanceForAsset(asset.to_string()))?;

        match amount_type {
            AmountType::Available => balance.available += amount,
            AmountType::Locked => balance.locked += amount,
        }

        Ok(())
    }

    /// Read-only access to a user's available balance for an asset. Used in tests.
    #[cfg(test)]
    pub fn get_available_balance(&self, user_id: &str, asset: &Asset) -> Option<Decimal> {
        let mutex = self.balances.get(user_id)?;
        let balances = mutex.lock().ok()?;
        balances.balance.get(asset).map(|a| a.available)
    }

    /// Read-only access to a user's locked balance for an asset. Used in tests.
    #[cfg(test)]
    pub fn get_locked_balance(&self, user_id: &str, asset: &Asset) -> Option<Decimal> {
        let mutex = self.balances.get(user_id)?;
        let balances = mutex.lock().ok()?;
        balances.balance.get(asset).map(|a| a.locked)
    }
}

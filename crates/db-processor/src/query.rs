use crate::types::{DbTrade, KlineData, TickerData};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::{Pool, Postgres};
use std::str::FromStr;

pub async fn insert_trade(pool: &Pool<Postgres>, trade: DbTrade) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO trades(
          trade_id, market, price, quantity, user_id, other_user_id, order_id, timestamp
      ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(trade.trade_id)
    .bind(trade.market)
    .bind(trade.price)
    .bind(trade.quantity)
    .bind(trade.user_id)
    .bind(trade.other_user_id)
    .bind(trade.order_id)
    .bind(trade.timestamp)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_trades_from_db(
    pool: &Pool<Postgres>,
    market: String,
) -> Result<Vec<DbTrade>, sqlx::Error> {
    let trades = sqlx::query!(
        "SELECT * FROM trades WHERE market = $1 ORDER BY timestamp desc LIMIT 100",
        market
    )
    .fetch_all(pool)
    .await?;

    let trades_vec: Vec<DbTrade> = trades
        .iter()
        .filter_map(|trade| {
            let price = Decimal::from_str(&trade.price.to_string()).ok()?;
            let quantity = Decimal::from_str(&trade.quantity.to_string()).ok()?;
            Some(DbTrade {
                trade_id: trade.trade_id,
                market: trade.market.clone(),
                price,
                quantity,
                user_id: trade.user_id.clone(),
                other_user_id: trade.other_user_id.clone(),
                order_id: trade.order_id.clone(),
                timestamp: trade.timestamp,
            })
        })
        .collect();

    Ok(trades_vec)
}

fn parse_custom_date(date_str: &str) -> Option<String> {
    let simplified = date_str.replace("+00:00:00", "+00:00");
    let fixed_offset =
        DateTime::parse_from_str(&simplified, "%Y-%m-%d %H:%M:%S%.f %:z").ok()?;
    Some(fixed_offset.with_timezone(&Utc).to_string())
}

pub async fn get_klines_timeseries_data(
    pool: &Pool<Postgres>,
    market: String,
    interval: String,
    start_time: String,
) -> Result<Vec<KlineData>, sqlx::Error> {
    let start_time_int: i64 = start_time.parse().unwrap_or(0);

    let klines = sqlx::query!(
        "
        WITH timeseries_data AS (
            SELECT
                date_trunc($1, to_timestamp(timestamp / 1000)) AS bucket,
                price,
                quantity,
                trade_id,
                to_timestamp(timestamp / 1000) AS trade_time,
                ROW_NUMBER() OVER (PARTITION BY date_trunc($1, to_timestamp(timestamp / 1000)) ORDER BY timestamp ASC) AS row_num_asc,
                ROW_NUMBER() OVER (PARTITION BY date_trunc($1, to_timestamp(timestamp / 1000)) ORDER BY timestamp DESC) AS row_num_desc
            FROM trades
            WHERE timestamp >= $2
              AND market = $3
        )
        , aggregated_data AS (
            SELECT
                bucket,
                MAX(price) AS high,
                MIN(price) AS low,
                SUM(quantity) AS volume,
                COUNT(trade_id) AS trades,
                SUM(price * quantity) AS quote_volume,
                MIN(trade_time) AS start_time,
                MAX(trade_time) AS end_time,
                MAX(CASE WHEN row_num_asc = 1 THEN price END) AS open,
                MAX(CASE WHEN row_num_desc = 1 THEN price END) AS close
            FROM timeseries_data
            GROUP BY bucket
        )
        SELECT
            open,
            bucket AS end,
            high,
            low,
            close,
            quote_volume,
            start_time AS start,
            trades,
            volume
        FROM aggregated_data
        ORDER BY bucket ASC
        ",
        interval,
        start_time_int,
        market
    )
    .fetch_all(pool)
    .await?;

    let kline_data_vec: Vec<KlineData> = klines
        .iter()
        .filter_map(|kline| {
            let start = parse_custom_date(&kline.start?.to_string())?;
            let end = parse_custom_date(&kline.end?.to_string())?;

            Some(KlineData {
                open: kline.open.as_ref()?.to_string(),
                high: kline.high.as_ref()?.to_string(),
                low: kline.low.as_ref()?.to_string(),
                close: kline.close.as_ref()?.to_string(),
                quote_volume: kline.quote_volume.as_ref()?.to_string(),
                start,
                end,
                trades: kline.trades?.to_string(),
                volume: kline.volume.as_ref()?.to_string(),
            })
        })
        .collect();

    Ok(kline_data_vec)
}

pub async fn get_tickers_from_db(pool: &Pool<Postgres>) -> Result<Vec<TickerData>, sqlx::Error> {
    let now = Utc::now();
    let start_time_24h_ago = now - chrono::Duration::hours(24);
    let start_timestamp_24h_ago = start_time_24h_ago.timestamp_millis();

    let tickers = sqlx::query!(
        "
        WITH aggregated_trades AS (
            SELECT
                market AS symbol,
                MIN(price) FILTER (WHERE row_num_asc = 1) AS first_price,
                MAX(price) AS high,
                MIN(price) AS low,
                MIN(price) FILTER (WHERE row_num_desc = 1) AS last_price,
                MAX(price) FILTER (WHERE row_num_desc = 1) - MIN(price) FILTER (WHERE row_num_asc = 1) AS price_change,
                (MAX(price) FILTER (WHERE row_num_desc = 1) - MIN(price) FILTER (WHERE row_num_asc = 1)) / MIN(price) FILTER (WHERE row_num_asc = 1) AS price_change_percent,
                SUM(quantity) AS volume,
                SUM(price * quantity) AS quote_volume,
                COUNT(trade_id) AS trades
            FROM (
                SELECT
                    market,
                    price,
                    quantity,
                    trade_id,
                    ROW_NUMBER() OVER (PARTITION BY market ORDER BY timestamp ASC) AS row_num_asc,
                    ROW_NUMBER() OVER (PARTITION BY market ORDER BY timestamp DESC) AS row_num_desc
                FROM trades
                WHERE timestamp >= $1
            ) trade_data
            GROUP BY market
        )
        SELECT 
            symbol, 
            first_price, 
            high, 
            low, 
            last_price, 
            price_change, 
            price_change_percent, 
            quote_volume, 
            trades, 
            volume
        FROM aggregated_trades
        ORDER BY symbol ASC;
        ",
        start_timestamp_24h_ago
    )
    .fetch_all(pool)
    .await?;

    let ticker_data: Vec<TickerData> = tickers
        .iter()
        .filter_map(|row| {
            Some(TickerData {
                symbol: row.symbol.clone().to_string(),
                first_price: row.first_price.as_ref()?.to_string(),
                high: row.high.as_ref()?.to_string(),
                low: row.low.as_ref()?.to_string(),
                last_price: row.last_price.as_ref()?.to_string(),
                price_change: row.price_change.as_ref()?.to_string(),
                price_change_percent: row.price_change_percent.as_ref()?.to_string(),
                quote_volume: row.quote_volume.as_ref()?.to_string(),
                trades: row.trades?.to_string(),
                volume: row.volume.as_ref()?.to_string(),
            })
        })
        .collect();

    Ok(ticker_data)
}

pub async fn get_latest_trade_id_from_db(
    pool: &Pool<Postgres>,
    market: String,
) -> Result<i64, sqlx::Error> {
    let latest_trade = sqlx::query!(
        "SELECT trade_id FROM trades WHERE market = $1 ORDER BY trade_id desc LIMIT 1",
        market
    )
    .fetch_optional(pool)
    .await?;

    Ok(latest_trade.map(|r| r.trade_id).unwrap_or(0))
}

// --- Open order persistence ---

pub async fn upsert_open_order(
    pool: &Pool<Postgres>,
    order_id: &str,
    user_id: &str,
    market: &str,
    side: &str,
    price: Decimal,
    quantity: Decimal,
    filled_quantity: Decimal,
    order_status: &str,
    timestamp: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO open_orders (order_id, user_id, market, side, price, quantity, filled_quantity, order_status, timestamp)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        ON CONFLICT (order_id) DO UPDATE SET
            filled_quantity = EXCLUDED.filled_quantity,
            order_status = EXCLUDED.order_status
        "#,
    )
    .bind(order_id)
    .bind(user_id)
    .bind(market)
    .bind(side)
    .bind(price)
    .bind(quantity)
    .bind(filled_quantity)
    .bind(order_status)
    .bind(timestamp)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_open_order(pool: &Pool<Postgres>, order_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM open_orders WHERE order_id = $1")
        .bind(order_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub struct OpenOrderRow {
    pub order_id: String,
    pub user_id: String,
    pub market: String,
    pub side: String,
    pub price: Decimal,
    pub quantity: Decimal,
    pub filled_quantity: Decimal,
    pub order_status: String,
    pub timestamp: i64,
}

pub async fn load_open_orders(pool: &Pool<Postgres>) -> Result<Vec<OpenOrderRow>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (String, String, String, String, sqlx::types::BigDecimal, sqlx::types::BigDecimal, sqlx::types::BigDecimal, String, i64)>(
        "SELECT order_id, user_id, market, side, price, quantity, filled_quantity, order_status, timestamp FROM open_orders"
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|(order_id, user_id, market, side, price, quantity, filled_quantity, order_status, timestamp)| {
            Some(OpenOrderRow {
                order_id,
                user_id,
                market,
                side,
                price: Decimal::from_str(&price.to_string()).ok()?,
                quantity: Decimal::from_str(&quantity.to_string()).ok()?,
                filled_quantity: Decimal::from_str(&filled_quantity.to_string()).ok()?,
                order_status,
                timestamp,
            })
        })
        .collect())
}

// --- User balance persistence ---

pub async fn upsert_user_balance(
    pool: &Pool<Postgres>,
    user_id: &str,
    asset: &str,
    available: Decimal,
    locked: Decimal,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO user_balances (user_id, asset, available, locked)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (user_id, asset) DO UPDATE SET
            available = EXCLUDED.available,
            locked = EXCLUDED.locked
        "#,
    )
    .bind(user_id)
    .bind(asset)
    .bind(available)
    .bind(locked)
    .execute(pool)
    .await?;
    Ok(())
}

pub struct UserBalanceRow {
    pub user_id: String,
    pub asset: String,
    pub available: Decimal,
    pub locked: Decimal,
}

pub async fn load_user_balances(
    pool: &Pool<Postgres>,
) -> Result<Vec<UserBalanceRow>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (String, String, sqlx::types::BigDecimal, sqlx::types::BigDecimal)>(
        "SELECT user_id, asset, available, locked FROM user_balances"
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|(user_id, asset, available, locked)| {
            Some(UserBalanceRow {
                user_id,
                asset,
                available: Decimal::from_str(&available.to_string()).ok()?,
                locked: Decimal::from_str(&locked.to_string()).ok()?,
            })
        })
        .collect())
}

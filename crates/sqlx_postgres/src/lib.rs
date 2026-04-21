use sqlx::postgres::PgPoolOptions;

pub struct PostgresDb {
    pool: sqlx::Pool<sqlx::Postgres>,
}

impl PostgresDb {
    pub async fn new() -> Result<Self, sqlx::Error> {
        let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(&db_url)
            .await?;

        tracing::info!("connected to postgres");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS trades (
                trade_id BIGINT PRIMARY KEY,
                market VARCHAR NOT NULL,
                price NUMERIC NOT NULL,
                quantity NUMERIC NOT NULL,
                user_id VARCHAR NOT NULL,
                other_user_id VARCHAR NOT NULL,
                order_id VARCHAR NOT NULL,
                timestamp BIGINT NOT NULL
            );
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS open_orders (
                order_id VARCHAR PRIMARY KEY,
                user_id VARCHAR NOT NULL,
                market VARCHAR NOT NULL,
                side VARCHAR NOT NULL,
                price NUMERIC NOT NULL,
                quantity NUMERIC NOT NULL,
                filled_quantity NUMERIC NOT NULL,
                order_status VARCHAR NOT NULL,
                timestamp BIGINT NOT NULL
            );
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_balances (
                user_id VARCHAR NOT NULL,
                asset VARCHAR NOT NULL,
                available NUMERIC NOT NULL,
                locked NUMERIC NOT NULL,
                PRIMARY KEY (user_id, asset)
            );
            "#,
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }

    pub fn get_pg_connection(&self) -> Result<sqlx::Pool<sqlx::Postgres>, sqlx::Error> {
        Ok(self.pool.clone())
    }
}

use anyhow::Result;
use sqlx::{Pool, Postgres};

pub async fn connect(database_url: &str) -> Result<Pool<Postgres>> {
    let pool = sqlx::PgPool::connect(database_url).await?;
    Ok(pool)
}

pub async fn migrate(pool: &Pool<Postgres>) -> Result<()> {
    sqlx::raw_sql(include_str!("../../migrations/0001_create_incomes.sql"))
        .execute(pool)
        .await?;
    Ok(())
}

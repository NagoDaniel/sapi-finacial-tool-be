use anyhow::Result;
use sqlx::{Pool, Postgres};

pub async fn connect(database_url: &str) -> Result<Pool<Postgres>> {
    let pool = sqlx::PgPool::connect(database_url).await?;
    Ok(pool)
}

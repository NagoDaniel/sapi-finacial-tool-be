use anyhow::{Context, Result};
use std::env;

#[derive(Clone, Debug)]
pub struct Settings {
    pub database_url: String,
    pub port: u16,
}

impl Settings {
    pub fn from_env() -> Result<Self> {
        let database_url = env::var("DATABASE_URL")
            .context("DATABASE_URL is required")?;
        let port = env::var("PORT")
            .unwrap_or_else(|_| "8080".to_string())
            .parse::<u16>()
            .context("PORT must be a valid port number")?;

        Ok(Self {
            database_url,
            port,
        })
    }
}

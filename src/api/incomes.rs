use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

const CATEGORIES: &[&str] = &["Salary", "Freelance", "Investments", "Business", "Other"];
const DEFAULT_PAGE_SIZE: u32 = 10;
const MAX_PAGE_SIZE: u32 = 100;

pub fn router() -> Router<PgPool> {
    Router::new()
        .route("/api/incomes", get(list_incomes).post(create_income))
        .route(
            "/api/incomes/:id",
            get(get_income).put(update_income).delete(delete_income),
        )
}

#[derive(Debug, Serialize, FromRow)]
pub struct Income {
    id: Uuid,
    amount_cents: i64,
    source: String,
    category: String,
    description: Option<String>,
    received_on: NaiveDate,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct IncomeInput {
    amount_cents: i64,
    source: String,
    category: String,
    description: Option<String>,
    received_on: NaiveDate,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct IncomeFilters {
    page: u32,
    per_page: u32,
    q: Option<String>,
    category: Option<String>,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    sort_by: String,
    sort_order: String,
}

impl Default for IncomeFilters {
    fn default() -> Self {
        Self {
            page: 1,
            per_page: DEFAULT_PAGE_SIZE,
            q: None,
            category: None,
            from: None,
            to: None,
            sort_by: "received_on".to_owned(),
            sort_order: "desc".to_owned(),
        }
    }
}

#[derive(Serialize)]
struct IncomeList {
    data: Vec<Income>,
    pagination: Pagination,
    total_amount_cents: i64,
}

#[derive(Serialize)]
struct Pagination {
    page: u32,
    per_page: u32,
    total_items: i64,
    total_pages: i64,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: message.into(),
        }
    }

    fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: "Income record not found".to_owned(),
        }
    }

    fn internal(error: sqlx::Error) -> Self {
        tracing::error!(%error, "Income request failed");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "The income request could not be completed".to_owned(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                error: self.message,
            }),
        )
            .into_response()
    }
}

fn validate_input(mut input: IncomeInput) -> Result<IncomeInput, ApiError> {
    input.source = input.source.trim().to_owned();
    input.category = input.category.trim().to_owned();
    input.description = input
        .description
        .map(|description| description.trim().to_owned())
        .filter(|description| !description.is_empty());

    if input.amount_cents <= 0 {
        return Err(ApiError::bad_request("Amount must be greater than zero"));
    }
    if input.source.is_empty() || input.source.len() > 120 {
        return Err(ApiError::bad_request(
            "Source must contain between 1 and 120 characters",
        ));
    }
    if !CATEGORIES.contains(&input.category.as_str()) {
        return Err(ApiError::bad_request("Choose a valid income category"));
    }
    if input
        .description
        .as_ref()
        .is_some_and(|description| description.len() > 500)
    {
        return Err(ApiError::bad_request(
            "Description must be at most 500 characters",
        ));
    }

    Ok(input)
}

fn validate_filters(filters: &IncomeFilters) -> Result<(), ApiError> {
    if filters.page == 0 {
        return Err(ApiError::bad_request("Page must be at least 1"));
    }
    if filters.per_page == 0 || filters.per_page > MAX_PAGE_SIZE {
        return Err(ApiError::bad_request("Page size must be between 1 and 100"));
    }
    if filters
        .category
        .as_ref()
        .is_some_and(|category| !CATEGORIES.contains(&category.as_str()))
    {
        return Err(ApiError::bad_request("Choose a valid income category"));
    }
    if filters
        .q
        .as_deref()
        .is_some_and(|search| search.chars().count() > 120)
    {
        return Err(ApiError::bad_request(
            "Search must be at most 120 characters",
        ));
    }
    if filters
        .from
        .zip(filters.to)
        .is_some_and(|(from, to)| from > to)
    {
        return Err(ApiError::bad_request(
            "The start date must be on or before the end date",
        ));
    }
    Ok(())
}

fn sort_column(sort_by: &str) -> Result<&'static str, ApiError> {
    match sort_by {
        "received_on" => Ok("received_on"),
        "amount" => Ok("amount_cents"),
        "source" => Ok("source"),
        "category" => Ok("category"),
        "created_at" => Ok("created_at"),
        _ => Err(ApiError::bad_request(
            "Sort by received_on, amount, source, category, or created_at",
        )),
    }
}

fn sort_direction(sort_order: &str) -> Result<&'static str, ApiError> {
    match sort_order {
        "asc" => Ok("ASC"),
        "desc" => Ok("DESC"),
        _ => Err(ApiError::bad_request("Sort order must be asc or desc")),
    }
}

async fn list_incomes(
    State(pool): State<PgPool>,
    Query(filters): Query<IncomeFilters>,
) -> Result<Json<IncomeList>, ApiError> {
    validate_filters(&filters)?;
    let sort_column = sort_column(&filters.sort_by)?;
    let sort_direction = sort_direction(&filters.sort_order)?;
    let category = filters.category.as_deref();
    let from = filters.from;
    let to = filters.to;
    let search = filters
        .q
        .as_deref()
        .map(str::trim)
        .filter(|search| !search.is_empty())
        .map(|search| format!("%{search}%"));
    let page_size = i64::from(filters.per_page);
    let offset = i64::from(filters.page - 1) * page_size;
    let where_clause = "
        ($1::text IS NULL OR category = $1)
        AND ($2::date IS NULL OR received_on >= $2)
        AND ($3::date IS NULL OR received_on <= $3)
        AND ($4::text IS NULL OR source ILIKE $4 OR COALESCE(description, '') ILIKE $4)
    ";

    let (total_items, total_amount_cents): (i64, i64) = sqlx::query_as(&format!(
        "SELECT COUNT(*), COALESCE(SUM(amount_cents), 0)::BIGINT FROM incomes WHERE {where_clause}"
    ))
    .bind(category)
    .bind(from)
    .bind(to)
    .bind(&search)
    .fetch_one(&pool)
    .await
    .map_err(ApiError::internal)?;

    let data = sqlx::query_as::<_, Income>(&format!(
        "SELECT id, amount_cents, source, category, description, received_on, created_at, updated_at
         FROM incomes WHERE {where_clause}
         ORDER BY {sort_column} {sort_direction}, id ASC
         LIMIT $5 OFFSET $6"
    ))
    .bind(category)
    .bind(from)
    .bind(to)
    .bind(search)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(Json(IncomeList {
        data,
        pagination: Pagination {
            page: filters.page,
            per_page: filters.per_page,
            total_items,
            total_pages: (total_items + page_size - 1) / page_size,
        },
        total_amount_cents,
    }))
}

async fn get_income(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
) -> Result<Json<Income>, ApiError> {
    sqlx::query_as::<_, Income>(
        "SELECT id, amount_cents, source, category, description, received_on, created_at, updated_at
         FROM incomes WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&pool)
    .await
    .map_err(ApiError::internal)?
    .map(Json)
    .ok_or_else(ApiError::not_found)
}

async fn create_income(
    State(pool): State<PgPool>,
    Json(input): Json<IncomeInput>,
) -> Result<(StatusCode, Json<Income>), ApiError> {
    let input = validate_input(input)?;
    let income = sqlx::query_as::<_, Income>(
        "INSERT INTO incomes (id, amount_cents, source, category, description, received_on)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, amount_cents, source, category, description, received_on, created_at, updated_at",
    )
    .bind(Uuid::new_v4())
    .bind(input.amount_cents)
    .bind(input.source)
    .bind(input.category)
    .bind(input.description)
    .bind(input.received_on)
    .fetch_one(&pool)
    .await
    .map_err(ApiError::internal)?;

    tracing::info!(
        income_id = %income.id,
        amount_cents = income.amount_cents,
        category = %income.category,
        "income_created"
    );
    Ok((StatusCode::CREATED, Json(income)))
}

async fn update_income(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
    Json(input): Json<IncomeInput>,
) -> Result<Json<Income>, ApiError> {
    let input = validate_input(input)?;
    let income = sqlx::query_as::<_, Income>(
        "UPDATE incomes
         SET amount_cents = $2, source = $3, category = $4, description = $5,
             received_on = $6, updated_at = NOW()
         WHERE id = $1
         RETURNING id, amount_cents, source, category, description, received_on, created_at, updated_at",
    )
    .bind(id)
    .bind(input.amount_cents)
    .bind(input.source)
    .bind(input.category)
    .bind(input.description)
    .bind(input.received_on)
    .fetch_optional(&pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;

    tracing::info!(
        income_id = %income.id,
        amount_cents = income.amount_cents,
        category = %income.category,
        "income_updated"
    );
    Ok(Json(income))
}

async fn delete_income(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let result = sqlx::query("DELETE FROM incomes WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .map_err(ApiError::internal)?;

    if result.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    tracing::info!(income_id = %id, "income_deleted");
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_input() -> IncomeInput {
        IncomeInput {
            amount_cents: 1250,
            source: "Consulting".to_owned(),
            category: "Freelance".to_owned(),
            description: Some("Client work".to_owned()),
            received_on: NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(),
        }
    }

    #[test]
    fn normalizes_valid_income_fields() {
        let mut valid = valid_input();
        valid.source = "  Consulting  ".to_owned();
        valid.category = " Freelance ".to_owned();
        valid.description = Some("  Client work  ".to_owned());

        let normalized = validate_input(valid).unwrap();
        assert_eq!(normalized.source, "Consulting");
        assert_eq!(normalized.category, "Freelance");
        assert_eq!(normalized.description.as_deref(), Some("Client work"));
        assert_eq!(normalized.amount_cents, 1250);
    }

    #[test]
    fn converts_blank_descriptions_to_none() {
        let mut input = valid_input();
        input.description = Some("  \n ".to_owned());

        let normalized = validate_input(input).unwrap();
        assert_eq!(normalized.description, None);
    }

    #[test]
    fn rejects_invalid_income_fields_with_unprocessable_entity() {
        let mut zero_amount = valid_input();
        zero_amount.amount_cents = 0;
        let mut negative_amount = valid_input();
        negative_amount.amount_cents = -1;
        let mut empty_source = valid_input();
        empty_source.source = "  ".to_owned();
        let mut long_source = valid_input();
        long_source.source = "s".repeat(121);
        let mut invalid_category = valid_input();
        invalid_category.category = "Unknown".to_owned();
        let mut long_description = valid_input();
        long_description.description = Some("n".repeat(501));

        for invalid in [
            zero_amount,
            negative_amount,
            empty_source,
            long_source,
            invalid_category,
            long_description,
        ] {
            let error = validate_input(invalid).unwrap_err();
            assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
        }
    }

    #[test]
    fn accepts_supported_sort_fields_and_orders() {
        for (requested, expected) in [
            ("received_on", "received_on"),
            ("amount", "amount_cents"),
            ("source", "source"),
            ("category", "category"),
            ("created_at", "created_at"),
        ] {
            assert_eq!(sort_column(requested).unwrap(), expected);
        }
        assert_eq!(sort_direction("asc").unwrap(), "ASC");
        assert_eq!(sort_direction("desc").unwrap(), "DESC");
    }

    #[test]
    fn rejects_untrusted_sort_fields_and_orders() {
        assert_eq!(sort_column("amount").unwrap(), "amount_cents");
        for invalid in ["id", "id; DROP TABLE incomes", "amount_cents"] {
            assert_eq!(
                sort_column(invalid).unwrap_err().status,
                StatusCode::UNPROCESSABLE_ENTITY
            );
        }
        for invalid in ["random", "DESC", "asc; DROP TABLE incomes"] {
            assert_eq!(
                sort_direction(invalid).unwrap_err().status,
                StatusCode::UNPROCESSABLE_ENTITY
            );
        }
    }

    #[test]
    fn accepts_valid_listing_filters_and_page_size_boundaries() {
        let first_page = IncomeFilters {
            per_page: 1,
            ..IncomeFilters::default()
        };
        assert!(validate_filters(&first_page).is_ok());

        let largest_page = IncomeFilters {
            per_page: MAX_PAGE_SIZE,
            ..IncomeFilters::default()
        };
        assert!(validate_filters(&largest_page).is_ok());
    }

    #[test]
    fn rejects_invalid_listing_filters() {
        let start = NaiveDate::from_ymd_opt(2026, 5, 2).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        let invalid_filters = [
            IncomeFilters {
                page: 0,
                ..IncomeFilters::default()
            },
            IncomeFilters {
                per_page: 0,
                ..IncomeFilters::default()
            },
            IncomeFilters {
                per_page: MAX_PAGE_SIZE + 1,
                ..IncomeFilters::default()
            },
            IncomeFilters {
                category: Some("Unknown".to_owned()),
                ..IncomeFilters::default()
            },
            IncomeFilters {
                q: Some("q".repeat(121)),
                ..IncomeFilters::default()
            },
            IncomeFilters {
                from: Some(start),
                to: Some(end),
                ..IncomeFilters::default()
            },
        ];

        for filters in invalid_filters {
            let error = validate_filters(&filters).unwrap_err();
            assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
        }
    }

    #[test]
    fn maps_missing_records_and_database_errors_to_expected_statuses() {
        assert_eq!(ApiError::not_found().status, StatusCode::NOT_FOUND);
        assert_eq!(
            ApiError::internal(sqlx::Error::PoolClosed).status,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}

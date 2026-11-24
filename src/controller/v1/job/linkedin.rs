use crate::utils::linkedin_scraper::{
    JobPosting, fetch_linkedin_job_detail, fetch_linkedin_jobs_in_location_page, linkedin_job_id,
};

use axum::{
    Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    path::Path as FsPath,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::task;

const PUBLIC_API_URL: &str = "https://api.ilyasuperglue.tech";
const JOB_CACHE_TTL_SECONDS: i64 = 12 * 60;
const JOB_CACHE_PATH: &str = "data/jobs.sqlite3";

fn open_cache() -> rusqlite::Result<Connection> {
    if let Some(parent) = FsPath::new(JOB_CACHE_PATH).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    }
    let connection = Connection::open(JOB_CACHE_PATH)?;
    connection.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA busy_timeout = 5000;
         CREATE TABLE IF NOT EXISTS job_cache (
            cache_key TEXT PRIMARY KEY,
            fetched_at INTEGER NOT NULL,
            jobs_json TEXT NOT NULL
        )",
    )?;
    Ok(connection)
}

fn current_timestamp() -> anyhow::Result<i64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64)
}

async fn read_cached_jobs(cache_key: &str) -> anyhow::Result<Option<Vec<JobPosting>>> {
    let cache_key = cache_key.to_owned();
    task::spawn_blocking(move || -> anyhow::Result<Option<Vec<JobPosting>>> {
        let connection = open_cache()?;
        let cutoff = current_timestamp()? - JOB_CACHE_TTL_SECONDS;
        let jobs_json = connection
            .query_row(
                "SELECT jobs_json FROM job_cache WHERE cache_key = ?1 AND fetched_at > ?2",
                params![cache_key, cutoff],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        jobs_json
            .map(|json| serde_json::from_str(&json).map_err(anyhow::Error::from))
            .transpose()
    })
    .await?
}

async fn write_cached_jobs(cache_key: &str, jobs: &[JobPosting]) -> anyhow::Result<()> {
    let cache_key = cache_key.to_owned();
    let jobs_json = serde_json::to_string(jobs)?;
    task::spawn_blocking(move || -> anyhow::Result<()> {
        let connection = open_cache()?;
        connection.execute(
            "INSERT INTO job_cache (cache_key, fetched_at, jobs_json) VALUES (?1, ?2, ?3)
             ON CONFLICT(cache_key) DO UPDATE SET fetched_at = excluded.fetched_at, jobs_json = excluded.jobs_json",
            params![cache_key, current_timestamp()?, jobs_json],
        )?;
        Ok(())
    })
    .await?
}

#[derive(Deserialize, utoipa::IntoParams)]
pub(crate) struct JobsQuery {
    #[param(default = "Malaysia")]
    pub(crate) country: Option<String>,
    #[param(default = "React Native")]
    pub(crate) search: Option<String>,
    #[param(default = 168)]
    pub(crate) hours_old: Option<u32>,
    #[param(default = 1)]
    pub(crate) page: Option<u32>,
    #[param(default = 20)]
    pub(crate) limit: Option<u32>,
}

fn country_location(country: &str) -> Option<&'static str> {
    match country.trim().to_ascii_lowercase().as_str() {
        "indonesia" | "id" => Some("Indonesia"),
        "malaysia" | "my" => Some("Malaysia"),
        "singapore" | "sg" => Some("Singapore"),
        "australia" | "au" => Some("Australia"),
        _ => None,
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ListedJob {
    #[serde(flatten)]
    job: JobPosting,
    check_detail: Option<String>,
    linkedin_url: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct Pagination {
    page: u32,
    limit: u32,
    has_more: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct JobsPage {
    jobs: Vec<ListedJob>,
    pagination: Pagination,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct JobDetail {
    id: String,
    description: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ErrorResponse {
    error: String,
}

impl IntoResponse for ErrorResponse {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::BAD_GATEWAY, Json(self)).into_response()
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ApiResponse<T: Serialize + utoipa::ToSchema> {
    success: bool,
    data: T,
}

impl<T: Serialize + utoipa::ToSchema> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> axum::response::Response {
        Json(self).into_response()
    }
}

#[utoipa::path(
    get,
    path = "/v1/jobs/linkedin",
    params(JobsQuery),
    responses(
        (status = 200, description = "LinkedIn jobs", body = ApiResponse<JobsPage>),
        (status = 400, description = "Invalid country", body = ErrorResponse),
        (status = 502, description = "Job source unavailable", body = ErrorResponse)
    )
)]
async fn get_jobs(
    State(state): State<crate::GlobalState>,
    Query(query): Query<JobsQuery>,
) -> Result<impl IntoResponse, axum::response::Response> {
    let location = match query.country.as_deref() {
        None => Some("Malaysia"),
        Some(country) => match country_location(country) {
            Some(location) => Some(location),
            None => {
                let error = ErrorResponse {
                    error: "country must be Indonesia, Malaysia, Singapore, or Australia".into(),
                };
                return Err((StatusCode::BAD_REQUEST, Json(error)).into_response());
            }
        },
    };
    let search = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&state.search_query);
    let hours_old = query.hours_old.unwrap_or(168);
    let page = query.page.unwrap_or(1);
    let limit = query.limit.unwrap_or(20);
    if page == 0 || !(1..=40).contains(&limit) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "page must be at least 1 and limit must be between 1 and 40".into(),
            }),
        )
            .into_response());
    }
    let cache_key =
        serde_json::to_string(&(search, location.unwrap_or_default(), hours_old, page, limit))
            .map_err(|error| {
                ErrorResponse {
                    error: format!("Failed to create cache key: {error}"),
                }
                .into_response()
            })?;
    let jobs = match read_cached_jobs(&cache_key).await {
        Ok(Some(jobs)) => jobs,
        Ok(None) => {
            let jobs = fetch_linkedin_jobs_in_location_page(
                search,
                limit as usize,
                location,
                Some(hours_old),
                page,
            )
            .await
            .map_err(|error| {
                ErrorResponse {
                    error: format!("Failed to fetch jobs: {error}"),
                }
                .into_response()
            })?;
            if let Err(error) = write_cached_jobs(&cache_key, &jobs).await {
                tracing::warn!(%error, "Failed to write jobs cache");
            }
            jobs
        }
        Err(error) => {
            tracing::warn!(%error, "Failed to read jobs cache");
            fetch_linkedin_jobs_in_location_page(
                search,
                limit as usize,
                location,
                Some(hours_old),
                page,
            )
            .await
            .map_err(|error| {
                ErrorResponse {
                    error: format!("Failed to fetch jobs: {error}"),
                }
                .into_response()
            })?
        }
    };
    let has_more = jobs.len() == limit as usize;
    let response = ApiResponse {
        success: true,
        data: JobsPage {
            jobs: jobs
                .into_iter()
                .map(|job| {
                    let check_detail = linkedin_job_id(&job.url)
                        .map(|id| format!("{PUBLIC_API_URL}/v1/jobs/linkedin/{id}"));
                    let linkedin_url = job.url.clone();
                    ListedJob {
                        job,
                        check_detail,
                        linkedin_url,
                    }
                })
                .collect(),
            pagination: Pagination {
                page,
                limit,
                has_more,
            },
        },
    };
    Ok(response)
}

#[utoipa::path(
    get,
    path = "/v1/jobs/linkedin/{id}",
    params(
        ("id" = String, Path, description = "LinkedIn job ID")
    ),
    responses(
        (status = 200, description = "Job description", body = ApiResponse<JobDetail>),
        (status = 400, description = "Invalid job ID", body = ErrorResponse),
        (status = 502, description = "Job source unavailable", body = ErrorResponse)
    )
)]
pub(crate) async fn get_job_detail(
    Path(id): Path<String>,
) -> Result<impl IntoResponse, axum::response::Response> {
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Job ID must contain only digits".into(),
            }),
        )
            .into_response());
    }
    let description = fetch_linkedin_job_detail(&id).await.map_err(|error| {
        ErrorResponse {
            error: format!("Failed to fetch job detail: {error}"),
        }
        .into_response()
    })?;
    Ok(ApiResponse {
        success: true,
        data: JobDetail { id, description },
    })
}

pub fn router() -> Router<crate::GlobalState> {
    Router::new().nest(
        "/jobs",
        Router::new()
            .route("/linkedin", get(get_jobs))
            .route("/linkedin/{id}", get(get_job_detail)),
    )
}

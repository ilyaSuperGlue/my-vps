use crate::utils::jobstreet_scraper::{
    JobStreetPosting, fetch_job_detail, fetch_jobs, validate_limit,
};
use axum::{
    Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
};
use serde::{Deserialize, Serialize};

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

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct JobStreetPagination {
    page: u32,
    limit: u32,
    total_count: u64,
    total_pages: u64,
    has_more: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct JobStreetJobsPage {
    jobs: Vec<JobStreetPosting>,
    pagination: JobStreetPagination,
}

#[derive(Deserialize, utoipa::IntoParams)]
pub(crate) struct JobStreetDetailQuery {
    #[param(default = "MY")]
    pub(crate) country: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct JobStreetErrorResponse {
    error: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct JobStreetApiResponse<T: Serialize + utoipa::ToSchema> {
    success: bool,
    data: T,
}

impl<T: Serialize + utoipa::ToSchema> IntoResponse for JobStreetApiResponse<T> {
    fn into_response(self) -> axum::response::Response {
        Json(self).into_response()
    }
}

#[utoipa::path(
    get,
    path = "/v1/jobs/jobstreet",
    params(JobsQuery),
    responses(
        (status = 200, description = "JobStreet jobs", body = JobStreetApiResponse<JobStreetJobsPage>),
        (status = 400, description = "Invalid parameters", body = JobStreetErrorResponse),
        (status = 502, description = "JobStreet unavailable", body = JobStreetErrorResponse)
    )
)]
async fn get_jobs(
    State(state): State<crate::GlobalState>,
    Query(query): Query<JobsQuery>,
) -> Result<impl IntoResponse, axum::response::Response> {
    let country = match query
        .country
        .as_deref()
        .unwrap_or("Malaysia")
        .trim()
        .to_ascii_uppercase()
        .as_str()
    {
        "MY" | "MALAYSIA" => "MY",
        "SG" | "SINGAPORE" => "SG",
        "ID" | "INDONESIA" => "ID",
        "PH" | "PHILIPPINES" => "PH",
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(JobStreetErrorResponse {
                    error: "country must be Malaysia, Singapore, Indonesia, or Philippines".into(),
                }),
            )
                .into_response());
        }
    };
    let search = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&state.search_query);
    let hours_old = query.hours_old.unwrap_or(168);
    let page = query.page.unwrap_or(1);
    let limit = query.limit.unwrap_or(20);
    if page == 0 || hours_old == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(JobStreetErrorResponse {
                error: "page and hours_old must be greater than 0".into(),
            }),
        )
            .into_response());
    }
    if let Err(error) = validate_limit(limit) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(JobStreetErrorResponse {
                error: error.to_string(),
            }),
        )
            .into_response());
    }

    let result = fetch_jobs(search, country, page, limit, hours_old)
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_GATEWAY,
                Json(JobStreetErrorResponse {
                    error: format!("Failed to fetch JobStreet jobs: {error}"),
                }),
            )
                .into_response()
        })?;
    let total_pages = if result.total_count == 0 {
        0
    } else {
        result.total_count.div_ceil(limit as u64)
    };
    let has_more = (page as u64) < total_pages;

    Ok(JobStreetApiResponse {
        success: true,
        data: JobStreetJobsPage {
            jobs: result.jobs,
            pagination: JobStreetPagination {
                page,
                limit,
                total_count: result.total_count,
                total_pages,
                has_more,
            },
        },
    })
}

#[utoipa::path(
    get,
    path = "/v1/jobs/jobstreet/{id}",
    params(
        ("id" = String, Path, description = "JobStreet job ID"),
        JobStreetDetailQuery
    ),
    responses(
        (status = 200, description = "Full JobStreet job detail", body = JobStreetApiResponse<serde_json::Value>),
        (status = 400, description = "Invalid parameters", body = JobStreetErrorResponse),
        (status = 502, description = "JobStreet unavailable", body = JobStreetErrorResponse)
    )
)]
async fn get_job_detail(
    Path(id): Path<String>,
    Query(query): Query<JobStreetDetailQuery>,
) -> Result<impl IntoResponse, axum::response::Response> {
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(JobStreetErrorResponse {
                error: "Job ID must contain only digits".into(),
            }),
        )
            .into_response());
    }
    let country = match query
        .country
        .as_deref()
        .unwrap_or("MY")
        .trim()
        .to_ascii_uppercase()
        .as_str()
    {
        "MY" | "MALAYSIA" => "MY",
        "SG" | "SINGAPORE" => "SG",
        "ID" | "INDONESIA" => "ID",
        "PH" | "PHILIPPINES" => "PH",
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(JobStreetErrorResponse {
                    error: "country must be Malaysia, Singapore, Indonesia, or Philippines".into(),
                }),
            )
                .into_response());
        }
    };
    let detail = fetch_job_detail(&id, country).await.map_err(|error| {
        (
            StatusCode::BAD_GATEWAY,
            Json(JobStreetErrorResponse {
                error: format!("Failed to fetch JobStreet job detail: {error}"),
            }),
        )
            .into_response()
    })?;
    Ok(JobStreetApiResponse {
        success: true,
        data: detail,
    })
}

pub(crate) fn router() -> Router<crate::GlobalState> {
    Router::new()
        .route("/jobs/jobstreet", get(get_jobs))
        .route("/jobs/jobstreet/{id}", get(get_job_detail))
}

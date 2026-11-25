mod job;
mod status;

use axum::Router;
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        status::get_server_status,
        job::linkedin::get_jobs,
        job::linkedin::get_job_detail,
        job::jobstreet::get_jobs,
        job::jobstreet::get_job_detail
    ),
    components(schemas(
        status::DiskInfo,
        status::ServerStatus,
        crate::utils::linkedin_scraper::LinkedInJobDetail,
        crate::utils::linkedin_scraper::JobCriterion,
        job::linkedin::ListedJob,
        job::linkedin::Pagination,
        job::linkedin::JobsPage,
        job::linkedin::JobDetail,
        job::linkedin::ErrorResponse,
        job::linkedin::ApiResponse<Vec<job::linkedin::ListedJob>>,
        job::linkedin::ApiResponse<job::linkedin::JobDetail>,
        crate::utils::linkedin_scraper::JobPosting,
        crate::utils::jobstreet_scraper::JobStreetPosting,
        job::jobstreet::JobStreetPagination,
        job::jobstreet::JobStreetJobsPage,
        job::jobstreet::JobStreetErrorResponse,
        job::jobstreet::JobStreetApiResponse<job::jobstreet::JobStreetJobsPage>
    ))
)]
pub struct ApiDoc;

#[derive(OpenApi)]
#[openapi(
    paths(
        job::linkedin::get_jobs,
        job::linkedin::get_job_detail,
        job::jobstreet::get_jobs,
        job::jobstreet::get_job_detail
    ),
    components(schemas(
        job::linkedin::ListedJob,
        job::linkedin::Pagination,
        job::linkedin::JobsPage,
        job::linkedin::JobDetail,
        job::linkedin::ErrorResponse,
        job::linkedin::ApiResponse<Vec<job::linkedin::ListedJob>>,
        job::linkedin::ApiResponse<job::linkedin::JobsPage>,
        job::linkedin::ApiResponse<job::linkedin::JobDetail>,
        crate::utils::linkedin_scraper::JobPosting,
        crate::utils::jobstreet_scraper::JobStreetPosting,
        job::jobstreet::JobStreetPagination,
        job::jobstreet::JobStreetJobsPage,
        job::jobstreet::JobStreetErrorResponse,
        job::jobstreet::JobStreetApiResponse<job::jobstreet::JobStreetJobsPage>
    ))
)]
pub struct JobsApiDoc;

pub fn router() -> Router<crate::GlobalState> {
    Router::new().merge(status::router()).merge(job::router())
}

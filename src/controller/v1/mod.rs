mod job;
mod status;

use axum::Router;

pub fn router() -> Router<crate::GlobalState> {
    Router::new().merge(status::router()).merge(job::router())
}

mod v1;
use axum::Router;
pub use v1::{ApiDoc, JobsApiDoc};

pub fn router() -> Router<crate::GlobalState> {
    let router = Router::new().nest("/v1", v1::router());
    router
}

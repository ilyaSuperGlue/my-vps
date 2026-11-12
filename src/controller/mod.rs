mod v1;
use axum::Router;

pub fn router() -> Router<crate::GlobalState> {
    let router = Router::new().nest("/v1", v1::router());
    router
}

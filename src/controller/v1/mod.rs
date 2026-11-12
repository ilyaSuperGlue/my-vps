mod status;

use axum::Router;

pub fn router() -> Router<crate::GlobalState> {
    let router = Router::new().merge(status::router());
    router
}

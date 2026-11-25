pub(crate) mod jobstreet;
pub mod linkedin;

use axum::Router;

pub fn router() -> Router<crate::GlobalState> {
    Router::new()
        .merge(linkedin::router())
        .merge(jobstreet::router())
}

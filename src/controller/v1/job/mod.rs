pub mod linkedin;

use axum::Router;

pub fn router() -> Router<crate::GlobalState> {
    linkedin::router()
}

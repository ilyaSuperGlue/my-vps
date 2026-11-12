mod constant;
mod controller;
mod middleware;
mod utils;

use axum::{
    Router,
    http::{StatusCode, Uri},
    middleware as mid,
    routing::get,
};
use std::sync::{Arc, Mutex};
use std::{net::SocketAddr, time::Duration};
use sysinfo::System;
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};
use tracing_appender::{non_blocking, rolling};

use crate::middleware::logging::{LogConfig, log_requests};

#[derive(Clone)]
pub struct GlobalState {
    sys: Arc<Mutex<System>>,
}

#[tokio::main]
async fn main() {
    let file_appender = rolling::daily("logs", "app.log");
    let (writer, _guard) = non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(writer)
        .with_ansi(false)
        .init();

    let governor_conf = GovernorConfigBuilder::default()
        .per_second(10)
        .burst_size(30)
        .finish()
        .unwrap();

    // a separate background task to clean up
    let governor_limiter = governor_conf.limiter().clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));

        loop {
            interval.tick().await;

            tracing::debug!("rate limiting storage size: {}", governor_limiter.len());

            governor_limiter.retain_recent();
        }
    });

    let mut sys = System::new_all();
    sys.refresh_all();

    let server_state = GlobalState {
        sys: Arc::new(Mutex::new(sys)),
    };

    async fn fallback(uri: Uri) -> (StatusCode, String) {
        (StatusCode::NOT_FOUND, format!("No route for {uri}"))
    }

    use serde::Serialize;

    #[derive(Serialize)]
    struct Health {
        health: String,
    }

    async fn health_check() -> axum::Json<Health> {
        let msg: Health = Health {
            health: "Ok!".to_string(),
        };
        axum::Json(msg)
    }

    let app = Router::new()
        .route("/", get(health_check))
        .merge(controller::router())
        .with_state(server_state)
        .layer(GovernorLayer::new(governor_conf))
        .route_layer(mid::from_fn_with_state(LogConfig::default(), log_requests))
        .fallback(fallback);

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

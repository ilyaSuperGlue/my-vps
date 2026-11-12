use std::{net::SocketAddr, time::Instant};

use axum::{
    extract::{ConnectInfo, MatchedPath, Request, State},
    http::{HeaderMap, HeaderValue, header},
    middleware::Next,
    response::Response,
};
use tracing::{error, info, warn};

const REQUEST_ID_HEADER: &str = "x-request-id";
const MAX_UA_LEN: usize = 256;

#[derive(Clone, Copy, Debug, Default)]
pub struct LogConfig {
    pub trust_proxy_headers: bool,
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn client_ip(headers: &HeaderMap, peer: SocketAddr, trust_proxy: bool) -> String {
    if trust_proxy {
        if let Some(first) = header_str(headers, "x-forwarded-for")
            .and_then(|xff| xff.split(',').next())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return first.to_string();
        }
        if let Some(ip) = header_str(headers, "x-real-ip")
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return ip.to_string();
        }
    }
    peer.ip().to_string()
}

pub async fn log_requests(
    State(cfg): State<LogConfig>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    mut req: Request,
    next: Next,
) -> Response {
    let start = Instant::now();

    let request_id = header_str(req.headers(), REQUEST_ID_HEADER)
        .map(str::to_owned)
        .unwrap_or_else(|| "unknown".to_string());
    if let Ok(v) = HeaderValue::from_str(&request_id) {
        req.headers_mut().insert(REQUEST_ID_HEADER, v);
    }

    let method = req.method().clone();
    let version = req.version();
    let path = req.uri().path().to_owned();
    // MatchedPath is only set when attached with `route_layer`.
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map(|m| m.as_str().to_owned())
        .unwrap_or_else(|| path.clone());

    let ip = client_ip(req.headers(), peer, cfg.trust_proxy_headers);
    let user_agent: String = header_str(req.headers(), header::USER_AGENT.as_str())
        .unwrap_or("-")
        .chars()
        .take(MAX_UA_LEN)
        .collect();
    let host = header_str(req.headers(), header::HOST.as_str())
        .unwrap_or("-")
        .to_owned();
    let referer = header_str(req.headers(), header::REFERER.as_str())
        .unwrap_or("-")
        .to_owned();
    let req_len = header_str(req.headers(), header::CONTENT_LENGTH.as_str())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    let mut res = next.run(req).await;

    let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
    let status = res.status();
    let res_len = header_str(res.headers(), header::CONTENT_LENGTH.as_str())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    if let Ok(v) = HeaderValue::from_str(&request_id) {
        res.headers_mut().insert(REQUEST_ID_HEADER, v);
    }

    macro_rules! emit {
        ($lvl:ident) => {
            $lvl!(
                request_id = %request_id,
                ip = %ip,
                method = %method,
                route = %route,
                path = %path,
                status = status.as_u16(),
                latency_ms = format_args!("{latency_ms:.2}"),
                user_agent = %user_agent,
                host = %host,
                referer = %referer,
                version = ?version,
                req_bytes = req_len,
                res_bytes = res_len,
                "request completed"
            )
        };
    }

    if status.is_server_error() {
        emit!(error);
    } else if status.is_client_error() {
        emit!(warn);
    } else {
        emit!(info);
    }

    res
}

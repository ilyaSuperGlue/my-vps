use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use sysinfo::{Disks, System};

use crate::GlobalState;
use crate::constant::unit::Unit;
use crate::utils::floor;

#[derive(Serialize)]
struct DiskInfo {
    name: String,
    total_space: f64,
    available_space: f64,
    mount_point: String,
}

#[derive(Serialize)]
struct ServerStatus {
    cpu_usage_percent: f64,
    total_memory: f64,
    used_memory: f64,
    total_swap: f64,
    used_swap: f64,
    disks: Option<DiskInfo>,
    unit: String,
    uptime: u64,
    name: Option<String>,
    os: Option<String>,
}

async fn get_server_status(
    State(state): State<GlobalState>,
    Query(query): Query<StatusQuery>,
) -> Json<ServerStatus> {
    let (unit, unit_name) = match query.unit.as_deref() {
        Some("KB") => (Unit::KB, "KB"),
        Some("MB") => (Unit::MB, "MB"),
        Some("GB") => (Unit::GB, "GB"),
        _ => (Unit::MB, "MB"),
    };
    let mut sys = state.sys.lock().unwrap();

    // Refresh the system information
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    let cpu_usage_percent = f64::from(sys.global_cpu_usage());
    let total_memory = unit.convert(sys.total_memory());
    let used_memory = unit.convert(sys.used_memory());
    let total_swap = unit.convert(sys.total_swap());
    let used_swap = unit.convert(sys.used_swap());

    let disks = Disks::new_with_refreshed_list()
        .list()
        .iter()
        .find(|disk| disk.mount_point().to_string_lossy() == "/")
        .map(|disk| DiskInfo {
            name: disk.name().to_string_lossy().to_string(),
            total_space: floor(unit.convert(disk.total_space())),
            available_space: floor(unit.convert(disk.available_space())),
            mount_point: disk.mount_point().to_string_lossy().to_string(),
        });

    Json(ServerStatus {
        cpu_usage_percent: floor(cpu_usage_percent),
        total_memory: floor(total_memory),
        used_memory: floor(used_memory),
        total_swap: floor(total_swap),
        used_swap: floor(used_swap),
        disks,
        unit: unit_name.to_string(),
        uptime: System::uptime(),
        name: System::name(),
        os: System::long_os_version(),
    })
}

#[derive(Debug, Deserialize)]
struct StatusQuery {
    #[serde(default)]
    unit: Option<String>,
}

pub fn router() -> Router<GlobalState> {
    Router::new().route("/status", get(get_server_status))
}

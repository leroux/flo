pub mod db;
pub mod models;
pub mod server;

pub fn version() -> String {
    format!("{}-{}", env!("CARGO_PKG_VERSION"), env!("FLO_BUILD_HASH"))
}

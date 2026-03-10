use sqlx::SqlitePool;
use tempfile::TempDir;

/// Fork the production database into a temp directory.
/// Returns (pool, _temp_dir). Hold onto _temp_dir — the DB is deleted when it drops.
pub async fn fork_db() -> (SqlitePool, TempDir) {
    let prod_db = dirs::home_dir()
        .expect("no home dir")
        .join(".flo")
        .join("flo.db");

    let tmp_dir = TempDir::new().expect("failed to create temp dir");
    let test_db = tmp_dir.path().join("flo-test.db");

    if prod_db.exists() {
        std::fs::copy(&prod_db, &test_db).expect("failed to copy production db");
        // Also copy WAL/SHM if they exist (SQLite WAL mode)
        let wal = prod_db.with_extension("db-wal");
        let shm = prod_db.with_extension("db-shm");
        if wal.exists() {
            std::fs::copy(&wal, test_db.with_extension("db-wal")).ok();
        }
        if shm.exists() {
            std::fs::copy(&shm, test_db.with_extension("db-shm")).ok();
        }
    }

    let db_url = format!("sqlite:{}?mode=rwc", test_db.display());
    let pool = SqlitePool::connect(&db_url)
        .await
        .expect("failed to connect to test db");

    // Run migrations so schema is up to date even if prod db didn't exist
    flo::db::init(&pool).await.expect("failed to init test db");

    (pool, tmp_dir)
}

/// Create a fresh empty test database (no production data).
pub async fn empty_db() -> (SqlitePool, TempDir) {
    let tmp_dir = TempDir::new().expect("failed to create temp dir");
    let test_db = tmp_dir.path().join("flo-test.db");

    let db_url = format!("sqlite:{}?mode=rwc", test_db.display());
    let pool = SqlitePool::connect(&db_url)
        .await
        .expect("failed to connect to test db");

    flo::db::init(&pool).await.expect("failed to init test db");

    (pool, tmp_dir)
}

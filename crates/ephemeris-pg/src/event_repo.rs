use deadpool_postgres::Pool;

#[allow(dead_code)]
pub struct PgEventRepository {
    pool: Pool,
}

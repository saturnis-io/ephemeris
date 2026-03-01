use deadpool_postgres::Pool;

#[allow(dead_code)]
pub struct PgAggregationRepository {
    pool: Pool,
}

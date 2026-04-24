use aura_core::effects::QueryEffects;
use aura_core::query::DatalogProgram;

async fn attempt_raw_query<H: QueryEffects>(handler: &H, program: &DatalogProgram) {
    let _ = handler.query_raw(program).await;
}

fn main() {}

use std::{error::Error, sync::Arc};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use clap::Parser;
use redis::{aio::MultiplexedConnection, AsyncCommands, Value};

#[derive(Parser)]
struct Cli {
    server_port: u16,
    redis_port: u16,
}

struct AppState {
    db_conn: MultiplexedConnection,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let server_port = cli.server_port;
    let redis_port = cli.redis_port;

    let db = redis::Client::open(format!("redis://127.0.0.1:{redis_port}"))?;
    let state = Arc::new(AppState {
        db_conn: db.get_multiplexed_async_connection().await?,
    });

    let app = Router::new()
        .route("/:id", get(redirect_to_id))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{server_port}")).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn redirect_to_id(Path(path): Path<String>, State(state): State<Arc<AppState>>) -> Response {
    let mut db_conn = state.db_conn.clone();
    match db_conn.get(path).await.unwrap() {
        Value::Nil => StatusCode::NOT_FOUND.into_response(),
        Value::SimpleString(url) => Redirect::to(&url).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

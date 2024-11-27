use std::{error::Error, sync::Arc};

use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use clap::Parser;
use redis::{aio::MultiplexedConnection, AsyncCommands, FromRedisValue, Value};
use serde::Deserialize;

#[derive(Parser)]
struct Cli {
    server_port: u16,
    redis_port: u16,
}

struct AppState {
    db_conn: MultiplexedConnection,
}

#[derive(Deserialize)]
struct AddRedirectParams {
    id: Option<String>,
    url: String,
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
        .route("/add", post(add_redirect))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{server_port}")).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn redirect_to_id(Path(path): Path<String>, State(state): State<Arc<AppState>>) -> Response {
    let mut db_conn = state.db_conn.clone();
    let db_val = db_conn.get(path).await.unwrap();

    match db_val {
        Value::Nil => StatusCode::NOT_FOUND.into_response(),
        _ => {
            let url: String = String::from_redis_value(&db_val).unwrap();
            Redirect::to(&url).into_response()
        }
    }
}

async fn get_from_db(key: &str, db_conn: &mut MultiplexedConnection) -> Option<String> {
    match db_conn.get(key).await.unwrap() {
        Value::Nil => None,
        Value::SimpleString(url) => Some(url),
        _ => None,
    }
}

async fn add_redirect(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AddRedirectParams>,
) -> Response {
    let mut db_conn = state.db_conn.clone();

    let id = match body.id {
        Some(id) => id,
        None => "random".to_string(),
    };

    if id == "add" {
        return (StatusCode::BAD_REQUEST, "Sorry, /add is reserved!").into_response();
    }

    match get_from_db(&id, &mut db_conn).await {
        Some(_) => (
            StatusCode::NOT_FOUND,
            "Your proposed short URL is already in use.",
        )
            .into_response(),
        None => match db_conn.set::<String, String, Value>(id, body.url).await {
            Ok(_) => StatusCode::CREATED.into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        },
    }
}

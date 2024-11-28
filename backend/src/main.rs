use rand::{distributions::Alphanumeric, thread_rng, Rng};
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
use serde::{Deserialize, Serialize};
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;

#[derive(Parser)]
struct Cli {
    server_port: u16,
    redis_url: String,
}

struct AppState {
    db_conn: MultiplexedConnection,
}

#[derive(Deserialize)]
struct AddRedirectBody {
    #[serde(default)]
    id: Option<String>,
    url: String,
}

#[derive(Serialize)]
struct AddResponse {
    id: String,
    // key is used for editing redirects
    key: String,
}

#[derive(Serialize)]
struct SimpleResponse {
    message: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let server_port = cli.server_port;
    let redis_url = match cli.redis_url.parse::<u16>() {
        Ok(port) => {
            println!("Warning: redis_url is a port, assuming localhost");
            format!("127.0.0.1:{port}")
        }
        Err(_) => cli.redis_url,
    };

    let db = redis::Client::open(format!("redis://{redis_url}"))?;
    let state = Arc::new(AppState {
        db_conn: db.get_multiplexed_async_connection().await?,
    });

    let app = Router::new()
        .route("/:id", get(redirect_to_id))
        .route("/add", post(add_redirect))
        .with_state(state)
        .layer(ServiceBuilder::new().layer(CorsLayer::permissive()));

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{server_port}")).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn redirect_to_id(Path(path): Path<String>, State(state): State<Arc<AppState>>) -> Response {
    let mut db_conn = state.db_conn.clone();
    match get_from_db(&path, &mut db_conn).await {
        Some(url) => Redirect::to(&url).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn get_from_db(key: &str, db_conn: &mut MultiplexedConnection) -> Option<String> {
    let db_val = db_conn.get(key).await.unwrap();
    match db_val {
        Value::Nil => None,
        _ => {
            let val = String::from_redis_value(&db_val).unwrap();
            Some(val)
        }
    }
}

fn generate_random_string() -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(10)
        .map(char::from)
        .collect()
}

async fn generate_random_id(db_conn: &mut MultiplexedConnection) -> String {
    let mut db_conn = db_conn.clone();

    loop {
        let id: String = generate_random_string();

        if get_from_db(&id, &mut db_conn).await.is_none() {
            return id;
        }
    }
}

async fn add_redirect(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AddRedirectBody>,
) -> Response {
    let mut db_conn = state.db_conn.clone();

    let id = match body.id {
        Some(id) => id,
        None => generate_random_id(&mut db_conn).await,
    };

    if id == "add" {
        return (StatusCode::BAD_REQUEST, "Sorry, /add is reserved!").into_response();
    }

    match get_from_db(&id, &mut db_conn).await {
        Some(_) => (
            StatusCode::CONFLICT,
            Json(SimpleResponse {
                message: "Your proposed short URL is already in use".to_string(),
            }),
        )
            .into_response(),
        None => match db_conn
            .set::<String, String, Value>(id.clone(), body.url)
            .await
        {
            Ok(_) => {
                let key = generate_random_string();
                db_conn
                    .set::<String, String, Value>(format!("key_{id}"), key.clone())
                    .await
                    .unwrap();
                (StatusCode::CREATED, Json(AddResponse { id, key })).into_response()
            }
            Err(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(SimpleResponse {
                    message: "Internal server error!".to_string(),
                }),
            )
                .into_response(),
        },
    }
}

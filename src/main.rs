use rand::{distributions::Alphanumeric, thread_rng, Rng};
use std::{error::Error, fs::File, io::Read, sync::Arc};

use axum::{
    extract::{Json, Path, State},
    http::{HeaderValue, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{delete, get, post},
    Router,
};
use clap::Parser;
use redis::{aio::MultiplexedConnection, AsyncCommands, FromRedisValue, Value};
use serde::{Deserialize, Serialize};
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;

#[derive(Parser)]
struct Cli {
    #[arg(env)]
    server_port: u16,
    #[arg(
        env,
        help = "If a port alone is provided, we will assume localhost:port."
    )]
    redis_url: String,

    #[arg(env, short, long, help = "Require this password to shorten new links")]
    password: Option<String>,

    #[arg(
        env,
        short,
        long,
        help = "The hostname of the server; ignore if using localhost.",
        name = "URL"
    )]
    server_hostname: Option<String>,

    #[arg(
        env,
        short,
        long,
        help = "The index.html you want to serve. Defaults to ./index.html.",
        name = "FILE_PATH"
    )]
    frontend_index: Option<String>,
}

struct AppState {
    db_conn: MultiplexedConnection,
    password: Option<String>,
    server_hostname: String,
    frontend_index: String,
}

#[derive(Deserialize)]
struct AddRedirectBody {
    #[serde(default)]
    id: Option<String>,
    url: String,
    #[serde(default)]
    password: Option<String>,
}

#[derive(Deserialize)]
struct DeleteRedirectBody {
    id: String,
    key: String,
    password: Option<String>,
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
        Ok(port) => format!("127.0.0.1:{port}"),
        Err(_) => cli.redis_url,
    };
    let server_hostname = match cli.server_hostname {
        Some(hostname) => hostname,
        None => format!("http://localhost:{server_port}").to_string(),
    };
    let frontend_index = match cli.frontend_index {
        Some(index) => index,
        None => "index.html".to_string(),
    };

    let db = redis::Client::open(format!("redis://{redis_url}"))?;
    let state = Arc::new(AppState {
        db_conn: db.get_multiplexed_async_connection().await?,
        password: cli.password,
        server_hostname,
        frontend_index,
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/:id", get(redirect_to_id))
        .route("/add", post(add_redirect))
        .route("/:id", delete(delete_redirect))
        .with_state(state)
        .layer(ServiceBuilder::new().layer(
            CorsLayer::new().allow_origin("{server_hostname}".parse::<HeaderValue>().unwrap()),
        ));

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{server_port}")).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index(State(state): State<Arc<AppState>>) -> Html<String> {
    let mut html = read_html_from_file(&state.frontend_index);
    let hostname = state.server_hostname.clone();
    html = html.replace("BACKEND_URL_HERE", &hostname);
    Html(html)
}

fn read_html_from_file(path: &str) -> String {
    let mut file = File::open(path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    contents
}

async fn redirect_to_id(Path(path): Path<String>, State(state): State<Arc<AppState>>) -> Response {
    let mut db_conn = state.db_conn.clone();
    match get_from_db(format!("redir_{path}").as_str(), &mut db_conn).await {
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

        if get_from_db(format!("redir_{id}").as_str(), &mut db_conn)
            .await
            .is_none()
        {
            return id;
        }
    }
}

fn check_password(
    password: Option<String>,
    provided_password: Option<String>,
) -> Result<(), Response> {
    if password.is_none() {
        Ok(())
    } else {
        match provided_password {
            Some(provided_password) => {
                if password.unwrap() == provided_password {
                    Ok(())
                } else {
                    Err((
                        StatusCode::UNAUTHORIZED,
                        Json(SimpleResponse {
                            message: "Password incorrect.".to_string(),
                        }),
                    )
                        .into_response())
                }
            }
            None => Err((
                StatusCode::BAD_REQUEST,
                Json(SimpleResponse {
                    message: "No password provided when one is required.".to_string(),
                }),
            )
                .into_response()),
        }
    }
}

async fn add_redirect(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AddRedirectBody>,
) -> Response {
    if let Err(e) = check_password(state.password.clone(), body.password) {
        return e;
    }

    let mut db_conn = state.db_conn.clone();

    let id = match body.id {
        Some(id) => id,
        None => generate_random_id(&mut db_conn).await,
    };

    if id == "add" {
        return (StatusCode::BAD_REQUEST, "Sorry, /add is reserved!").into_response();
    }

    match get_from_db(format!("redir_{id}").as_str(), &mut db_conn).await {
        Some(_) => (
            StatusCode::CONFLICT,
            Json(SimpleResponse {
                message: "Your proposed short URL is already in use".to_string(),
            }),
        )
            .into_response(),
        None => match db_conn
            .set::<String, String, Value>(format!("redir_{id}"), body.url)
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

async fn delete_redirect(
    State(state): State<Arc<AppState>>,
    Json(body): Json<DeleteRedirectBody>,
) -> Response {
    if let Err(e) = check_password(state.password.clone(), body.password) {
        return e;
    }

    let mut db_conn = state.db_conn.clone();
    let id = body.id;
    let provided_key = body.key;
    match get_from_db(format!("redir_{id}").as_str(), &mut db_conn).await {
        Some(_) => match get_from_db(&format!("key_{id}"), &mut db_conn).await {
            Some(key) => {
                if key != provided_key {
                    (
                        StatusCode::UNAUTHORIZED,
                        Json(SimpleResponse {
                            message: "Invalid key provided.".to_string(),
                        }),
                    )
                        .into_response()
                } else {
                    db_conn
                        .del::<String, Value>(format!("key_{id}"))
                        .await
                        .unwrap();
                    db_conn
                        .del::<String, Value>(format!("redir_{id}"))
                        .await
                        .unwrap();
                    (
                        StatusCode::OK,
                        Json(SimpleResponse {
                            message: "Short URL removed".to_string(),
                        }),
                    )
                        .into_response()
                }
            }
            None => (
                StatusCode::NOT_FOUND,
                Json(SimpleResponse {
                    message: "No key is associated with this short URL".to_string(),
                }),
            )
                .into_response(),
        },
        None => (
            StatusCode::NOT_FOUND,
            Json(SimpleResponse {
                message: "Short URL not found".to_string(),
            }),
        )
            .into_response(),
    }
}

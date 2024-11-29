use askama_axum::Template;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use std::{error::Error, sync::Arc};

use axum::{
    extract::{Json, Path, Query, State},
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Router,
};
use clap::Parser;
use redis::{aio::MultiplexedConnection, from_redis_value, AsyncCommands, FromRedisValue, Value};
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
        long,
        help = "The index.html you want to serve. Defaults to ./index.html.",
        name = "INDEX_HTML_PATH"
    )]
    index: Option<String>,

    #[arg(
        env,
        long,
        help = "The admin.html you want to serve. Defaults to ./admin.html.",
        name = "ADMIN_HTML_PATH"
    )]
    admin: Option<String>,

    #[arg(
        env,
        short,
        long,
        help = "The password for the admin page. Defaults to --password if it is set, otherwise None."
    )]
    admin_password: Option<String>,
}

struct AppState {
    db_conn: MultiplexedConnection,
    password: Option<String>,
    server_hostname: String,
    admin_password: Option<String>,
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

#[derive(Deserialize)]
struct AuthenticatedQuery {
    password: String,
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

#[derive(Serialize)]
struct Redirect {
    id: String,
    url: String,
    key: String,
}

impl FromRedisValue for Redirect {
    fn from_redis_value(v: &Value) -> redis::RedisResult<Self> {
        let redirect_string = String::from_redis_value(v)?;
        let parts: Vec<&str> = redirect_string.split('🧙').collect();
        Ok(Redirect {
            id: parts[0].to_string(),
            url: parts[1].to_string(),
            key: parts[2].to_string(),
        })
    }
}

#[derive(Template)]
#[template(path = "index/index.html")]
struct IndexTemplate<'a> {
    backend_url: &'a str,
    authentication_required: bool,
}

#[derive(Template)]
#[template(path = "admin/admin.html")]
struct AdminTemplate<'a> {
    backend_url: &'a str,
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
    let password = cli.password;
    let admin_password = match cli.admin_password {
        Some(admin_password) => Some(admin_password),
        None => password.clone(),
    };

    let db = redis::Client::open(format!("redis://{redis_url}"))?;
    let state = Arc::new(AppState {
        db_conn: db.get_multiplexed_async_connection().await?,
        password,
        server_hostname,
        admin_password,
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/admin", get(admin))
        .route("/:id", get(redirect_to_id))
        .route("/add", post(add_redirect))
        .route("/all", get(get_all_redirects))
        .route("/:id", delete(delete_redirect))
        .with_state(state)
        .layer(ServiceBuilder::new().layer(
            CorsLayer::new().allow_origin("{server_hostname}".parse::<HeaderValue>().unwrap()),
        ));

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{server_port}")).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index(State(state): State<Arc<AppState>>) -> Response {
    IndexTemplate {
        backend_url: &state.server_hostname.clone(),
        authentication_required: state.password.is_some(),
    }
    .into_response()
}

async fn admin(State(state): State<Arc<AppState>>) -> Response {
    if state.admin_password.is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    AdminTemplate {
        backend_url: &state.server_hostname,
    }
    .into_response()
}

async fn redirect_to_id(Path(path): Path<String>, State(state): State<Arc<AppState>>) -> Response {
    let mut db_conn = state.db_conn.clone();
    match get_from_db(format!("redir_{path}").as_str(), &mut db_conn).await {
        Some(url) => axum::response::Redirect::to(&url).into_response(),
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
    let url = body.url;

    let reserved_ids = ["add", "all", "admin"];
    if reserved_ids.contains(&id.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            format!("Sorry, /{id} is reserved!"),
        )
            .into_response();
    }

    if id.contains("🧙") {
        return (StatusCode::BAD_REQUEST, "🧙 is reserved... sorry...").into_response();
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
            .set::<String, String, Value>(format!("redir_{id}"), url.clone())
            .await
        {
            Ok(_) => {
                let key = generate_random_string();
                db_conn
                    .set::<String, String, Value>(format!("key_{id}"), key.clone())
                    .await
                    .unwrap();
                db_conn
                    // WSV - wizard separated values
                    .sadd::<&str, String, Value>("redirs", format!("{id}🧙{url}🧙{key}"))
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

async fn get_all_redirects(
    State(state): State<Arc<AppState>>,
    query: Query<AuthenticatedQuery>,
) -> Response {
    let mut db_conn = state.db_conn.clone();
    let password: String = query.password.clone();
    if state.admin_password.is_none() || password != state.admin_password.clone().unwrap() {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    return match db_conn.smembers::<&str, Value>("redirs").await.unwrap() {
        Value::Array(redirects) => {
            let redirects: Vec<Redirect> = redirects
                .iter()
                .map(|r| from_redis_value(r).unwrap())
                .collect();
            (StatusCode::OK, Json(redirects)).into_response()
        }
        Value::Nil => (
            StatusCode::NOT_FOUND,
            Json(SimpleResponse {
                message: "No redirects found.".to_string(),
            }),
        )
            .into_response(),
        val => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(SimpleResponse {
                message: format!("Got {val:?}, expected an Array or Nil"),
            }),
        )
            .into_response(),
    };
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
        Some(url) => match get_from_db(&format!("key_{id}"), &mut db_conn).await {
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
                    db_conn
                        .srem::<&str, String, Value>("redirs", format!("{id}🧙{url}🧙{key}"))
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

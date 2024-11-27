use axum::{extract::Path, routing::get, Router};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let app = Router::new().route("/:id", get(redirect_to_id));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn redirect_to_id(Path(path): Path<String>) -> String {
    path
}

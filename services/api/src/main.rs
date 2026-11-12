use axum::{routing::get,Json,Router}; use serde::Serialize;
#[derive(Serialize)] struct Health{status:&'static str}
async fn health()->Json<Health>{Json(Health{status:"ok"})}
#[tokio::main] async fn main(){let app=Router::new().route("/health",get(health));let listener=tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();axum::serve(listener,app).await.unwrap();}
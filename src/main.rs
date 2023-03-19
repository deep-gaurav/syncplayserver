use std::sync::Arc;

use async_graphql::{
    http::{playground_source, GraphQLPlaygroundConfig},
    Schema,
};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse, GraphQLSubscription};
use axum::{
    http::Method,
    response::{self, IntoResponse},
    routing::get,
    Extension, Router, Server,
};
use dashmap::DashMap;
use data::Storage;
use schema::{MutationRoot, QueryRoot, Subscription};
use tower_http::cors::{Any, CorsLayer};

pub mod data;
pub mod schema;
pub mod utils;

type MainSchema = Schema<QueryRoot, MutationRoot, Subscription>;

async fn graphql_handler(schema: Extension<MainSchema>, req: GraphQLRequest) -> GraphQLResponse {
    schema.execute(req.into_inner()).await.into()
}

async fn graphql_playground() -> impl IntoResponse {
    response::Html(playground_source(
        GraphQLPlaygroundConfig::new("/").subscription_endpoint("/ws"),
    ))
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    pretty_env_logger::init();
    let private_rooms = Arc::new(DashMap::new());
    let data = Storage {
        private_rooms: private_rooms,
    };

    let schema = Schema::build(QueryRoot, MutationRoot, Subscription)
        .data(data)
        .finish();
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        // allow requests from any origin
        .allow_origin(Any);

    let app = Router::new()
        .route("/", get(graphql_playground).post(graphql_handler))
        .route_service("/ws", GraphQLSubscription::new(schema.clone()))
        .layer(Extension(schema))
        .layer(cors);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let server_proc =
        Server::bind(&format!("0.0.0.0:{}", port).parse().unwrap()).serve(app.into_make_service());
    Ok(server_proc.await?)
}

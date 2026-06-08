mod search;

use std::{
    convert::Infallible,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
};
use futures::stream::Stream;
use futures::StreamExt;
use satisfactory_world_generator::{game::World, search_template::WorldSearchTemplate};
use search::{SearchEvent, SearchRequest, run_parallel_search};
use tokio::sync::{Mutex, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
};

#[derive(Clone)]
struct AppState {
    search_template: Arc<WorldSearchTemplate>,
    active_cancel: Arc<Mutex<Arc<AtomicBool>>>,
}

#[tokio::main]
async fn main() {
    let static_dir =
        std::env::var("STATIC_DIR").unwrap_or_else(|_| "dist".to_string());
    let addr = std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());

    let default_world: World =
        serde_json::from_str(include_str!("../../src/default-world.json"))
            .expect("default-world.json must be valid");

    let state = AppState {
        search_template: Arc::new(WorldSearchTemplate::from_world(&default_world)),
        active_cancel: Arc::new(Mutex::new(Arc::new(AtomicBool::new(false)))),
    };

    let index_path = format!("{static_dir}/index.html");
    let api = Router::new()
        .route("/api/search", post(search_handler))
        .route("/api/health", get(health_handler))
        .with_state(state);

    let app = Router::new()
        .merge(api)
        .fallback_service(
            ServeDir::new(&static_dir).not_found_service(ServeFile::new(index_path)),
        )
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|err| panic!("failed to bind {addr}: {err}"));

    println!("listening on http://{addr} (static files from {static_dir})");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .expect("server error");
}

async fn health_handler() -> &'static str {
    "ok"
}

async fn search_handler(
    State(state): State<AppState>,
    axum::Json(request): axum::Json<SearchRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut active = state.active_cancel.lock().await;
        active.store(true, Ordering::SeqCst);
        *active = Arc::clone(&cancel);
    }

    let search_template = Arc::clone(&state.search_template);

    let (tx, rx) = mpsc::channel::<SearchEvent>(32);

    tokio::task::spawn_blocking(move || {
        run_parallel_search(&search_template, &request, cancel, |event| {
            if tx.blocking_send(event).is_err() {
                // client disconnected
            }
        });
    });

    let stream = ReceiverStream::new(rx).map(move |event| {
        let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string());
        Ok(Event::default().data(data))
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

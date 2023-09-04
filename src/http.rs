use axum::debug_handler;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router, Server};
use serde_json::json;

use crate::procs::{start_procs, SharedRunningProcs};
use crate::sig::parse_signum;
use crate::spec::{Input, ProcId};

//------------------------------------------------------------------------------

pub enum Error {
    Request(String),
    NoSignal(String),
    Proc(crate::procs::Error),
}

impl From<crate::procs::Error> for Error {
    fn from(err: crate::procs::Error) -> Error {
        Error::Proc(err)
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            Error::Request(msg) => (StatusCode::BAD_REQUEST, msg),
            Error::NoSignal(signal) => (StatusCode::BAD_REQUEST, format!("no signal: {}", signal)),
            Error::Proc(crate::procs::Error::Io(err)) => {
                (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
            }
            Error::Proc(crate::procs::Error::NoProcId(proc_id)) => {
                (StatusCode::NOT_FOUND, format!("no proc id: {}", proc_id))
            }
            Error::Proc(crate::procs::Error::ProcRunning(proc_id)) => (
                StatusCode::BAD_REQUEST,
                format!("proc running: {}", proc_id),
            ),
            Error::Proc(crate::procs::Error::ProcNotRunning(proc_id)) => (
                StatusCode::BAD_REQUEST,
                format!("proc not running: {}", proc_id),
            ),
        };
        let body = json!({
            "status": status.to_string(),
            "error": msg,
        });
        (status, Json(body)).into_response()
    }
}

/// Handles `GET /procs`.
async fn procs_get(State(procs): State<SharedRunningProcs>) -> impl IntoResponse {
    Json(json!({
        "data": procs.to_result()
    }))
}

/// Handles `POST /procs`.
#[debug_handler]
async fn procs_post(
    State(procs): State<SharedRunningProcs>,
    Json(input): Json<Input>,
) -> Result<impl IntoResponse, Error> {
    // FIXME: Check duplicate proc IDs.
    start_procs(input, procs.clone()).await;
    let body = json!({
        "data": {
            // FIXME: Return a result.
        }
    });
    Ok((StatusCode::CREATED, Json(body)))
}

/// Handles `GET /procs/:id`.
async fn procs_id_get(
    State(procs): State<SharedRunningProcs>,
    Path(proc_id): Path<ProcId>,
) -> Result<impl IntoResponse, Error> {
    let proc = procs.get(&proc_id)?;
    let res = proc.lock().unwrap().to_result();
    let body = json!({
        "data": res
    });
    Ok(Json(body))
}

/// Handles `DEL /procs/:id`.
async fn procs_id_delete(
    State(procs): State<SharedRunningProcs>,
    Path(proc_id): Path<ProcId>,
) -> Result<impl IntoResponse, Error> {
    procs.remove_if_complete(&proc_id)?;
    let body = json!({"data": {
        // FIXME
    }});
    Ok(Json(body))
}

/// Handles POST /procs/:id/signal/:signum.
async fn procs_signal_signum_post(
    State(procs): State<SharedRunningProcs>,
    Path((proc_id, signum)): Path<(ProcId, String)>,
) -> Result<impl IntoResponse, Error> {
    let proc = procs.get(&proc_id)?;
    // FIXME: This is not an appropriate error.
    let signum = parse_signum(&signum).ok_or_else(|| Error::NoSignal(signum))?;
    proc.lock().unwrap().send_signal(signum)?;
    let body = json!({"data": {
        // FIXME
    }});
    Ok(Json(body))
}

//------------------------------------------------------------------------------

/// Runs the HTTP service.
pub async fn run_http(procs: SharedRunningProcs) -> Result<(), Box<dyn std::error::Error>> {
    let addr: std::net::SocketAddr = ([127, 0, 0, 1], 3000).into();
    // let listener = tokio::net::TcpListener::bind(addr).await?;

    let app = Router::new()
        .route("/procs", get(procs_get).post(procs_post))
        .route("/procs/:id", get(procs_id_get).delete(procs_id_delete))
        .route("/procs/:id/signals/:signum", post(procs_signal_signum_post))
        .with_state(procs);

    Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
    eprintln!("Listening on http://{}", addr);

    Ok(())
}

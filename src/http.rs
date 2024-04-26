use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::header::HeaderValue;
use hyper::{Method, Request, Response, StatusCode};
use log::*;
use serde_json::json;
use std::rc::Rc;

use crate::err::Error;
use crate::fd::{parse_fd, FdData};
use crate::procs::{start_procs, SharedProcs};
use crate::sig::parse_signum;
use crate::spec::{CaptureEncoding, Input, ProcId};

//------------------------------------------------------------------------------

type Req = Request<Incoming>;
type Rsp = Response<Full<Bytes>>;

struct RspError(StatusCode, Option<String>);

impl RspError {
    fn bad_request(msg: &str) -> RspError {
        RspError(StatusCode::BAD_REQUEST, Some(msg.to_string()))
    }
}

// type RspResult = Result<Rsp, RspError>;
type JsonResult = Result<serde_json::Value, RspError>;

// FIXME: All this is too complicated.  The handlers should just return a Rsp.

fn wrap_error(status: StatusCode, msg: Option<String>) -> serde_json::Value {
    json!({
        "errors": {
            "status": status.to_string(),
            "detail": msg,
        }
    })
}

fn json_response(rsp: JsonResult) -> Rsp {
    let (status, data) = match rsp {
        Ok(data) => (StatusCode::OK, json!({"data": data})),
        Err(RspError(status, msg)) => (status, wrap_error(status, msg)),
    };

    let (status, json) = match serde_json::to_string(&data) {
        Ok(body) => (status, body),
        Err(error) => {
            let status = StatusCode::INTERNAL_SERVER_ERROR;
            let body = serde_json::to_string(&wrap_error(status, Some(error.to_string()))).unwrap();
            (status, body)
        }
    };
    Response::builder()
        .status(status)
        .header(
            hyper::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )
        .body(Full::<Bytes>::from(json))
        .unwrap()
}

//------------------------------------------------------------------------------

/// Handles `GET /procs`.
async fn procs_get(procs: SharedProcs) -> JsonResult {
    Ok(json!({
        "procs": procs.to_result(),
    }))
}

/// Handles `GET /procs/:id`.
async fn procs_id_get(procs: SharedProcs, proc_id: &str) -> JsonResult {
    if let Some(proc) = procs.get(proc_id) {
        Ok(json!({
            "procs": {
                proc_id: proc.borrow().to_result(),
            }
        }))
    } else {
        Err(RspError(StatusCode::NOT_FOUND, None))
    }
}

/// Handles `DELETE /procs/:id`.
async fn procs_id_delete(procs: SharedProcs, proc_id: &str) -> JsonResult {
    let proc_id: ProcId = proc_id.to_string();
    match procs.remove_if_not_running(&proc_id) {
        Ok(_) => {
            Ok(json!({
                // FIXME
            }))
        }
        Err(Error::NoProcId(_)) => Err(RspError(StatusCode::NOT_FOUND, None)),
        Err(err) => Err(RspError::bad_request(&err.to_string())),
    }
}

/// Handles `POST /procs`.
async fn procs_post(procs: SharedProcs, input: Input) -> JsonResult {
    // FIXME: Check duplicate proc IDs.
    if let Err(err) = start_procs(&input.specs, &procs) {
        Err(RspError::bad_request(&err.to_string()))
    } else {
        Ok(json!({
            // FIXME
        }))
    }
}

/// Handles POST /procs/:id/signal/:signum.
async fn procs_signal_signum_post(procs: SharedProcs, proc_id: &str, signum: &str) -> JsonResult {
    let signum = parse_signum(signum).ok_or_else(|| RspError::bad_request("unknwon signum"))?;
    let proc = procs
        .get(proc_id)
        .ok_or_else(|| RspError(StatusCode::NOT_FOUND, None))?;
    proc.borrow()
        .send_signal(signum)
        .map_err(|e| RspError::bad_request(&e.to_string()))?;
    Ok(json!({
        // FIXME
    }))
}

/// Handles GET /procs/:id/output/:fd/data
async fn procs_output_data_get(procs: SharedProcs, proc_id: &str, fd: &str) -> Rsp {
    let fd = match parse_fd(fd) {
        Ok(fd) => fd,
        Err(err) => return json_response(Err(RspError::bad_request(&err.to_string()))),
    };
    let proc = match procs.get(proc_id) {
        Some(proc) => proc,
        None => return json_response(Err(RspError(StatusCode::NOT_FOUND, None))),
    };
    let fd_data = match proc.borrow().get_fd_data(fd, 0, None) {
        Ok(Some(fd_data)) => fd_data,
        Ok(None) => FdData::empty(),
        Err(err) => return json_response(Err(RspError::bad_request(&err.to_string()))),
    };
    assert!(fd_data.compression.is_none()); // FIXME: Indicate compresison in header.
    Response::builder()
        .status(200)
        .header(
            hyper::header::CONTENT_TYPE,
            match fd_data.encoding {
                None => "application/octet-stream",
                Some(CaptureEncoding::Utf8) => "text/plain; charset=utf-8",
            },
        )
        .body(Full::<Bytes>::from(fd_data.data))
        .unwrap()
}

//------------------------------------------------------------------------------

struct Router {
    router: matchit::Router<i32>,
}

impl Router {
    fn new() -> Router {
        let mut router = matchit::Router::new();
        router.insert("/procs", 0).unwrap();
        router.insert("/procs/:id", 1).unwrap();
        router.insert("/procs/:id/signals/:signum", 2).unwrap();
        router.insert("/procs/:id/output/:fd/data", 3).unwrap();
        Router { router }
    }

    async fn get_body_json(
        parts: &http::request::Parts,
        body: Incoming,
    ) -> Result<Input, RspError> {
        let bytes = body
            .collect()
            .await
            .map_err(|err| RspError::bad_request(&format!("reading body: {}", err)))?
            .to_bytes();
        let content_type = parts.headers.get("content-type");
        if content_type.is_none() {
            Err(RspError::bad_request("body not JSON"))
        } else if content_type.unwrap() != "application/json" {
            Err(RspError::bad_request("body not JSON"))
        } else if bytes.len() == 0 {
            Err(RspError::bad_request("no body"))
        } else {
            Ok(serde_json::from_slice::<Input>(&bytes)
                .map_err(|err| RspError::bad_request(&format!("parsing body: {}", err)))?)
        }
    }

    async fn dispatch(&self, req: Req, procs: SharedProcs) -> Rsp {
        let (parts, body) = req.into_parts();
        match self.router.at(parts.uri.path()) {
            Ok(m) => {
                let param = |p| m.params.get(p).unwrap();
                match (m.value, parts.method.clone()) {
                    // Match route numbers obtained from the matchit router, and
                    // methods.
                    (0, Method::GET) => json_response(procs_get(procs).await),
                    (0, Method::POST) => {
                        let input = match Router::get_body_json(&parts, body).await {
                            Ok(input) => input,
                            Err(error) => return json_response(Err(error)),
                        };
                        json_response(procs_post(procs, input).await)
                    }
                    (1, Method::GET) => json_response(procs_id_get(procs, param("id")).await),
                    (1, Method::DELETE) => json_response(procs_id_delete(procs, param("id")).await),

                    (2, Method::POST) => json_response(
                        procs_signal_signum_post(procs, param("id"), param("signum")).await,
                    ),

                    (3, Method::GET) => {
                        procs_output_data_get(procs, param("id"), param("fd")).await
                    }
                    // Route number (i.e. path match) but no method match.
                    (_, _) => json_response(Err(RspError(StatusCode::METHOD_NOT_ALLOWED, None))),
                }
            }

            // No path match.
            Err(_) => json_response(Err(RspError(StatusCode::NOT_FOUND, None))),
        }
    }
}

//------------------------------------------------------------------------------

/// Runs the HTTP service.
pub async fn run_http(procs: SharedProcs, port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let addr: std::net::SocketAddr = ([127, 0, 0, 1], port).into();

    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("Listening on http://{}", addr);

    // We match the URI path to (internal) route numbers, and dispatch below on
    // these.  It's a bit cumbersome to maintain, but quite efficient, and gives
    // us lots of flexibility in structuring the route handlers.
    let router = Rc::new(Router::new());

    loop {
        let (stream, _) = listener.accept().await?;
        let procs = procs.clone();
        let router = router.clone();

        let service = hyper::service::service_fn(move |req: Req| {
            let procs = procs.clone();
            let router = router.clone();
            async move {
                let rsp = router.dispatch(req, procs).await;
                // FIXME: https://jsonapi.org/format
                Ok::<Rsp, hyper::Error>(rsp)
            }
        });

        tokio::task::spawn_local(async move {
            if let Err(err) = hyper::server::conn::http1::Builder::new()
                .serve_connection(stream, service)
                .await
            {
                println!("Error serving connection: {:?}", err);
            }
        });
    }
}

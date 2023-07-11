use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::header::HeaderValue;
use hyper::{Method, Request, Response, StatusCode};
use serde_json::json;
use std::rc::Rc;

use crate::procs::{start_procs, SharedRunningProcs};
use crate::sig::parse_signum;
use crate::spec::{Input, ProcId};

//------------------------------------------------------------------------------

type Req = Request<Incoming>;
type Rsp = Response<Full<Bytes>>;

struct RspError(StatusCode, Option<String>);

impl RspError {
    fn bad_request(msg: &str) -> RspError {
        RspError(StatusCode::BAD_REQUEST, Some(msg.to_string()))
    }
}

type RspResult = Result<serde_json::Value, RspError>;

fn make_json_response<T>(status: StatusCode, obj: T) -> Rsp
where
    T: serde::Serialize,
{
    let body = serde_json::to_string(&obj).unwrap();
    Response::builder()
        .status(status)
        .header(
            hyper::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )
        .body(Full::<Bytes>::from(body))
        .unwrap()
}

fn make_response(rsp: RspResult) -> Rsp {
    match rsp {
        Ok(data) => make_json_response(
            StatusCode::OK,
            json!({
                "data": data,
            }),
        ),

        Err(err) => {
            let RspError(status, msg) = err;
            make_json_response(
                status,
                json!({
                    "errors": json!([
                        {
                            "status": status.to_string(),
                            "detail": msg,
                        },
                    ]),
                }),
            )
        }
    }
}

/// Handles `GET /procs`.
async fn procs_get(procs: SharedRunningProcs) -> RspResult {
    Ok(json!({
        "procs": procs.to_result(),
    }))
}

/// Handles `GET /procs/:id`.
async fn procs_id_get(procs: SharedRunningProcs, proc_id: &str) -> RspResult {
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

/// Handles `DEL /procs/:id`.
async fn procs_id_delete(procs: SharedRunningProcs, proc_id: &str) -> RspResult {
    let proc_id: ProcId = proc_id.to_string();
    match procs.remove_if_complete(&proc_id) {
        Ok(_) => {
            Ok(json!({
                // FIXME
            }))
        },
        Err(crate::procs::Error::NoProcId(_)) => Err(RspError(StatusCode::NOT_FOUND, None)),
        Err(err) => Err(RspError::bad_request(&err.to_string())),
    }
}

/// Handles `POST /procs`.
async fn procs_post(procs: SharedRunningProcs, input: Input) -> RspResult {
    // FIXME: Check duplicate proc IDs.
    start_procs(input, procs.clone()).await;
    Ok(json!({
        // FIXME
    }))
}

/// Handles POST /procs/:id/signal/:signum.
async fn procs_signal_signum_post(procs: SharedRunningProcs, proc_id: &str, signum: &str) -> RspResult {
    let signum = parse_signum(signum).ok_or_else(|| RspError::bad_request("unknwon signum"))?;
    let proc = procs.get(proc_id).ok_or_else(|| RspError(StatusCode::NOT_FOUND, None))?;
    proc.borrow().send_signal(signum).map_err(|e| RspError::bad_request(&e.to_string()))?;
    Ok(json!({
        // FIXME
    }))
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

    async fn dispatch(&self, req: Req, procs: SharedRunningProcs) -> RspResult {
        let (parts, body) = req.into_parts();
        let rsp = match self.router.at(parts.uri.path()) {
            Ok(m) => {
                let param = |p| m.params.get(p).unwrap();
                match (m.value, parts.method.clone()) {
                    // Match route numbers obtained from the matchit
                    // router, and methods.
                    (0, Method::GET) => procs_get(procs).await?,
                    (0, Method::POST) => {
                        let body = Router::get_body_json(&parts, body).await?;
                        procs_post(procs, body).await?
                    }
                    (1, Method::GET) => procs_id_get(procs, param("id")).await?,
                    (1, Method::DELETE) => procs_id_delete(procs, param("id")).await?,

                    (2, Method::POST) => procs_signal_signum_post(procs, param("id"), param("signum")).await?,

                    // Route number (i.e. path match) but no method match.
                    (_, _) => {
                        return Err(RspError(StatusCode::METHOD_NOT_ALLOWED, None));
                    }
                }
            }

            // No path match.
            Err(_) => {
                return Err(RspError(StatusCode::NOT_FOUND, None));
            }
        };

        Ok(rsp)
    }
}

//------------------------------------------------------------------------------

/// Runs the HTTP service.
pub async fn run_http(procs: SharedRunningProcs) -> Result<(), Box<dyn std::error::Error>> {
    let addr: std::net::SocketAddr = ([127, 0, 0, 1], 3000).into();

    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("Listening on http://{}", addr);

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
                Ok::<Rsp, hyper::Error>(make_response(rsp))
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

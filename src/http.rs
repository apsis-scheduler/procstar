use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::header::HeaderValue;
use hyper::{Method, Request, Response, StatusCode};
use serde_json::json;
use std::rc::Rc;

use crate::procs::{start_procs, SharedRunningProcs};
use crate::spec::Input;

//------------------------------------------------------------------------------

type Req = Request<Incoming>;
type Rsp = Response<Full<Bytes>>;

struct RspError(StatusCode, Option<String>);
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
    match procs.get(proc_id) {
        Some(proc) => Ok(json!({
            "proc": proc.borrow().to_result()
        })),
        None => Err(RspError(StatusCode::NOT_FOUND, None)),
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

//------------------------------------------------------------------------------

struct Router {
    router: matchit::Router<i32>,
}

impl Router {
    fn new() -> Router {
        let mut router = matchit::Router::new();
        router.insert("/procs", 0).unwrap();
        router.insert("/procs/:id", 1).unwrap();
        Router { router }
    }

    async fn get_body_json(body: Incoming) -> Result<Option<Input>, String> {
        let bytes = body
            .collect()
            .await
            .map_err(|err| format!("reading body: {}", err))?
            .to_bytes();
        if bytes.len() == 0 {
            Ok(None)
        } else {
            let jso = serde_json::from_slice::<Input>(&bytes)
                .map_err(|err| format!("parsing body: {}", err))?;
            Ok(Some(jso))
        }
    }

    async fn dispatch(&self, req: Req, procs: SharedRunningProcs) -> RspResult {
        let (req_parts, body) = req.into_parts();
        let body = Router::get_body_json(body)
            .await
            .map_err(|err| RspError(StatusCode::BAD_REQUEST, Some(err)))?;
        let rsp = match self.router.at(req_parts.uri.path()) {
            Ok(m) => {
                let param = |p| m.params.get(p).unwrap();
                match (m.value, req_parts.method) {
                    // Match route numbers obtained from the matchit
                    // router, and methods.
                    (0, Method::GET) => procs_get(procs).await?,
                    (0, Method::POST) => procs_post(procs, body.unwrap()).await?, // FIXME: unwrap
                    (1, Method::GET) => procs_id_get(procs, param("id")).await?,

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

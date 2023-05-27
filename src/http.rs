use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::header::HeaderValue;
use hyper::{Method, Request, Response, StatusCode};
use matchit::Router;
use serde_json::json;
use std::rc::Rc;

use crate::procs::SharedRunningProcs;

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

/// Handles `GET /procs`.
async fn procs_get(procs: SharedRunningProcs) -> RspResult {
    Ok(json!({
        "procs": procs.to_result(),
    }))
}

/// Handles `GET /procs/:id`.
async fn procs_id_get(procs: SharedRunningProcs, proc_id: &str) -> RspResult {
    match procs.get(proc_id) {
        Some(proc) => Ok(
            json!({
                "proc": proc.borrow().to_result()
            })),
        None => Err(RspError(StatusCode::NOT_FOUND, None)),
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
    let router = Rc::new({
        let mut router = Router::new();
        router.insert("/procs", 0)?;
        router.insert("/procs/:id", 1)?;
        router
    });

    loop {
        let (stream, _) = listener.accept().await?;
        let procs = procs.clone();
        let router = router.clone();

        let service = hyper::service::service_fn(move |req: Req| {
            let procs = procs.clone();
            let router = router.clone();

            async move {
                let rsp = match router.at(req.uri().path()) {
                    Ok(m) => {
                        let param = |p| m.params.get(p).unwrap();
                        match (m.value, req.method()) {
                            // Match route numbers obtained from the matchit
                            // router, and methods.
                            (0, &Method::GET) => procs_get(procs).await,
                            (1, &Method::GET) => procs_id_get(procs, param("id")).await,

                            // Route number (i.e. path match) but no method match.
                            (_, _) => Err(RspError(StatusCode::METHOD_NOT_ALLOWED, None)),
                        }
                    },

                    // No path match.
                    Err(_) => Err(RspError(StatusCode::NOT_FOUND, None)),
                };

                // FIXME: https://jsonapi.org/format

                Ok::<Rsp, hyper::Error>(match rsp {
                    Ok(d) =>
                        // Wrap the successful response.
                        make_json_response(StatusCode::OK, json!({
                            "data": d,
                        })),
                    Err(e) => {
                        // Wrap the error response.
                        let RspError(status, msg) = e;
                        make_json_response(status, json!({
                            "errors": json!([
                                {
                                    "status": status.to_string(),
                                    "detail": msg,
                                },
                            ]),
                        }))
                    },
                })
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

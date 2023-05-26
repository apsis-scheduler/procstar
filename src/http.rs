use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::header::HeaderValue;
use hyper::{Method, Request, Response, StatusCode};
use matchit::{Router, Params};
use std::rc::Rc;

use crate::procs::SharedRunningProcs;

//------------------------------------------------------------------------------

type Req = Request<Incoming>;
type Rsp = Response<Full<Bytes>>;

struct RspError(StatusCode, Option<String>);
type RspResult = Result<Rsp, RspError>;

fn make_json_response<T>(status: StatusCode, obj: T) -> Rsp
where
    T: serde::Serialize,
{
    let body = serde_json::to_string(&obj).unwrap();
    Response::builder()
        .status(status)
        .header(hyper::header::CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Full::<Bytes>::from(body))
        .unwrap()
}

fn make_error_response(req: &Req, status: StatusCode, msg: Option<&str>) -> Rsp {
    let body = serde_json::json!({
        "error": {
            "status": status.to_string(),
            "method": req.method().to_string(),
            "url": req.uri().to_string(),
            "message": msg,
        }
    }).to_string();
    Response::builder()
        .status(status)
        .body(Full::<Bytes>::from(body))
        .unwrap()
}

async fn procs_get(procs: SharedRunningProcs) -> RspResult {
    Ok(make_json_response(StatusCode::OK, procs.to_result()))
}

async fn procs_id_get(procs: SharedRunningProcs, proc_id: &str) -> RspResult {
    match procs.get(proc_id) {
        Some(proc) =>
            Ok(make_json_response(StatusCode::OK, procs.to_result())),
        None =>
            Err(RspError(StatusCode::NOT_FOUND, None)),
    }
}

//------------------------------------------------------------------------------

/// Runs the HTTP service.
pub async fn run_http(procs: SharedRunningProcs) -> Result<(), Box<dyn std::error::Error>> {
    let addr: std::net::SocketAddr = ([127, 0, 0, 1], 3000).into();

    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("Listening on http://{}", addr);

    // We match the URI path to (internal) route numbers, and dispatch later on
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
                let m = router.at(req.uri().path());

                // Dispatch on the route number and HTTP method.
                match (m.map(|m| (m.value, m.params)), req.method()) {
                    (Ok((0, _)), &Method::GET) => procs_get(procs).await,
                    (Ok((1, p)), &Method::GET) => procs_id_get(procs, p.get("id").unwrap()).await,

                    (Ok(_), _) =>
                        Ok(make_error_response(&req, StatusCode::METHOD_NOT_ALLOWED, None)),

                    (_, _) =>
                        Ok(make_error_response(&req, StatusCode::NOT_FOUND, None)),
                }
                // If we got a RspError, convert this to a nice JSON response
                // with corresponding HTTP status.
                .or_else::<hyper::Error, _>(|err| {
                    let RspError(status, msg) = err;
                    Ok(make_error_response(&req, status, msg.as_ref().map(|s| s.as_str())))
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

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

fn make_json_response<T>(status: StatusCode, obj: T) -> Result<Rsp, hyper::Error>
where
    T: serde::Serialize,
{
    let body = serde_json::to_string(&obj).unwrap();
    Ok(Response::builder()
        .status(status)
        .header(hyper::header::CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Full::<Bytes>::from(body))
        .unwrap())
}

fn make_error_response(req: Req, status: StatusCode, msg: Option<&str>) -> Result<Rsp, hyper::Error> {
    let body = serde_json::json!({
        "error": {
            "status": status.to_string(),
            "method": req.method().to_string(),
            "path": req.uri().path(),
            "message": msg,
        }
    }).to_string();
    Ok(Response::builder()
        .status(status)
        .body(Full::<Bytes>::from(body))
        .unwrap())
}

async fn procs_get(procs: SharedRunningProcs) -> Result<Rsp, hyper::Error> {
    make_json_response(StatusCode::OK, procs.to_result())
}

//------------------------------------------------------------------------------

/// Runs the HTTP service.
pub async fn run_http(procs: SharedRunningProcs) -> Result<(), Box<dyn std::error::Error>> {
    let addr: std::net::SocketAddr = ([127, 0, 0, 1], 3000).into();

    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("Listening on http://{}", addr);

    let router = Rc::new({
        let mut router = Router::new();
        router.insert("/procs", 0)?;
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
                let mtch = router.at(req.uri().path());
                let route_num = mtch.map(|m| m.value);
                match (route_num, req.method()) {
                    (Ok(0), &Method::GET) => procs_get(procs).await,

                    (Ok(_), _) =>
                        make_error_response(req, StatusCode::METHOD_NOT_ALLOWED, None),

                    (_, _) =>
                        make_error_response(req, StatusCode::NOT_FOUND, None),
                }
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

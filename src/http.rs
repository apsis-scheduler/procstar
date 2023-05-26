use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::{Request, Response, StatusCode, Method};

use crate::procs::SharedRunningProcs;

//------------------------------------------------------------------------------

type Req = Request<Incoming>;
// FIXME: Wrap JSON response in a body type.  Set content-type.
type Rsp = Response<Full<Bytes>>;

async fn procs_get(procs: SharedRunningProcs, _req: Req) -> Result<Rsp, hyper::Error> {
    let res = procs.to_result();
    let body = serde_json::to_string(&res).unwrap();
    Ok::<_, hyper::Error>(hyper::Response::new(Full::<Bytes>::from(body)))
}

fn error(req: Req) -> Result<Rsp, hyper::Error> {
    Ok(Response::builder()
       .status(StatusCode::NOT_FOUND)
       .body(Full::<Bytes>::from(req.uri().to_string()))
       .unwrap())
}

//------------------------------------------------------------------------------

/// Runs the HTTP service.
pub async fn run_http(procs: SharedRunningProcs) -> Result<(), Box<dyn std::error::Error>> {
    let addr: std::net::SocketAddr = ([127, 0, 0, 1], 3000).into();

    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("Listening on http://{}", addr);
    loop {
        let (stream, _) = listener.accept().await?;
        let procs = procs.clone();

        let service = hyper::service::service_fn(move |req: Req| {
            let procs = procs.clone();
            async move {
                match (req.uri().path(), req.method()) {
                    ("/procs", &Method::GET) => procs_get(procs, req).await,
                    _ => error(req),
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

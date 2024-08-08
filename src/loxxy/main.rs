use axum::{
    body::Body,
    extract::{Request, State},
    http::uri::Uri,
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use clap::Parser;
use clap_derive::ValueEnum;
use hyper::StatusCode;
use hyper_util::{client::legacy::connect::HttpConnector, rt::TokioExecutor};
use std::process::exit;
use tracing::{debug, info};

type Client = hyper_util::client::legacy::Client<HttpConnector, Body>;

#[derive(Debug, Clone, ValueEnum)]
enum Authentication {
    None,
    Basic,
    Rabbit,
    Oauth,
}

#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "http://loki-loki-gateway:80")]
    loki_url: String,

    #[arg(short, long)]
    auth: Authentication,

    #[arg(short, long, required_if_eq_any([("auth", "Authentication::Basic"), ("auth", "Authentication::Rabbit"), ("auth", "Authentication::Oauth")]
    ))]
    user: Option<String>,

    #[arg(short, long)]
    token: Option<String>,
}

#[derive(Clone)]
pub struct Statey {
    args: Args,
    client: Client,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    env_logger::init();

    // check that auth has been passed in
    match &args.auth {
        Authentication::None => {}
        Authentication::Basic | Authentication::Oauth | Authentication::Rabbit => {
            match (&args.user, &args.token) {
                (Some(_username), Some(_token)) => {}
                _ => {
                    info!("Authentication details, required.");
                    exit(1)
                }
            }
        }
    }

    info!("Starting Loxxy with args {:?}", args);
    let client: Client =
        hyper_util::client::legacy::Client::<(), ()>::builder(TokioExecutor::new())
            .build(HttpConnector::new());

    let state = Statey { args, client };
    let app = Router::new().route("/*0", post(handler)).with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4000").await.unwrap();
    info!("Loxxy listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

async fn handler(State(state): State<Statey>, mut req: Request) -> Result<Response, StatusCode> {
    let path = req.uri().path();
    let path_query = req
        .uri()
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or(path);

    let uri = format!("{}{}", state.args.loki_url, path_query);
    debug!("uri:: {}", uri);

    *req.uri_mut() = Uri::try_from(uri).unwrap();

    Ok(state
        .client
        .request(req)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?
        .into_response())
}

use axum::{
    body::Body,
    extract::{Request, State},
    http::uri::Uri,
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use clap::Parser;
use clap_derive::{Subcommand, ValueEnum};
use http_body_util::BodyExt;
use hyper::StatusCode;
use hyper_util::{client::legacy::connect::HttpConnector, rt::TokioExecutor};
use lapin::options::{BasicPublishOptions, ExchangeDeclareOptions};
use lapin::types::FieldTable;
use lapin::{BasicProperties, Channel, Connection, ConnectionProperties, ExchangeKind};

use oxxy::shapes::Client;
use paho_mqtt as mqtt;
use paho_mqtt::Message;
use std::process::exit;

use tracing::{debug, info};

#[derive(Debug, Clone, ValueEnum)]
enum Authentication {
    None,
    Basic,
    Rabbit,
    Oauth,
}

#[derive(Clone, Debug, Subcommand)]
enum Commands {
    HTTP {
        #[arg(short, long, default_value = "http://loki-loki-gateway:80")]
        loki_uri: String,
    },
    AMQP {
        #[clap(short, long)]
        rmq_uri: String,

        #[clap(short, long, default_value = "logging")]
        exchange: String,

        #[clap(short, long, default_value = "logs")]
        queue: String,

        #[clap(long, default_value = "logs")]
        routing_key: String,
    },
    MQTT {
        #[clap(short, long)]
        mqtt_uri: String,
        #[arg(short, long)]
        user: Option<String>,
        #[arg(short, long)]
        token: Option<String>,
        #[arg(long)]
        topic: String,
        #[arg(short, long, default_value = "0")]
        qos: usize,
    },
}

#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    cmd: Commands,

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
    amqp: Option<Channel>,
    mqtt: Option<mqtt::AsyncClient>,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
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
    let argsc = args.clone();
    let mut state = Statey {
        args: argsc,
        client,
        amqp: None,
        mqtt: None,
    };
    match &args.cmd {
        Commands::HTTP { .. } => {}
        Commands::AMQP {
            rmq_uri,
            exchange,
            queue: _,
            routing_key: _,
        } => {
            let options = ConnectionProperties::default();
            let connection = Connection::connect(rmq_uri, options).await.unwrap();
            let channel = connection.create_channel().await.unwrap();
            channel
                .exchange_declare(
                    exchange,                          // exchange name
                    ExchangeKind::Direct,              // type of exchange
                    ExchangeDeclareOptions::default(), // default options
                    FieldTable::default(),             // no extra parameters
                )
                .await
                .expect("Failed to declare channel");
            info!("mqtt connected");
            state.amqp = Some(channel);
        }
        Commands::MQTT {
            mqtt_uri,
            user,
            token,
            topic: _,
            qos: _,
        } => {
            let create_opts = mqtt::CreateOptionsBuilder::new()
                .server_uri(mqtt_uri)
                .client_id("oxxy-toxxy") // Set a client ID for your connection
                .finalize();
            let cli = mqtt::AsyncClient::new(create_opts)?;

            let conn_opts = match (user, token) {
                (Some(user), Some(token)) => {
                    mqtt::ConnectOptionsBuilder::new()
                        .user_name(user) // Set the username
                        .password(token) // Set the password
                        .clean_session(true) // Set clean session
                        .finalize()
                }
                _ => mqtt::ConnectOptionsBuilder::new().finalize(),
            };

            if let Err(e) = cli.connect(conn_opts).await {
                eprintln!("Unable to connect: {:?}", e);
                return Ok(());
            }

            state.mqtt = Some(cli)
        }
    }

    let app = match &args.cmd {
        Commands::HTTP { .. } => Router::new()
            .route("/*0", post(handler_http))
            .with_state(state),
        Commands::AMQP { .. } => Router::new()
            .route("/*0", post(handler_amqp))
            .with_state(state),
        Commands::MQTT { .. } => Router::new()
            .route("/*0", post(handler_mqtt))
            .with_state(state),
    };

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4000").await?;
    info!("Loxxy listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await
}

async fn handler_http(
    State(state): State<Statey>,
    mut req: Request,
) -> Result<Response, StatusCode> {
    let path = req.uri().path();
    let path_query = req
        .uri()
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or(path);
    match &state.args.cmd {
        Commands::HTTP { loki_uri: loki_url } => {
            let uri = format!("{}{}", loki_url, path_query);
            debug!("uri:: {}", uri);

            *req.uri_mut() = Uri::try_from(uri).unwrap();

            let resp = state
                .client
                .request(req)
                .await
                .map_err(|_| StatusCode::BAD_REQUEST)?
                .into_response();
            Ok(resp)
        }
        _ => Ok(Default::default()),
    }
}

async fn handler_amqp(State(state): State<Statey>, body: Body) -> Result<Response, StatusCode> {
    if let Commands::AMQP {
        rmq_uri: _,
        exchange,
        queue,
        routing_key: _,
    } = &state.args.cmd
    {
        let bodydata = body.collect().await.unwrap().to_bytes();
        debug!("{:?}", bodydata);
        let channel = state.amqp.unwrap();
        channel
            .basic_publish(
                exchange,
                queue,
                BasicPublishOptions::default(),
                &bodydata,
                BasicProperties::default(),
            )
            .await
            .expect("");
    }

    Ok(Default::default())
}

async fn handler_mqtt(State(state): State<Statey>, body: Body) -> Result<Response, StatusCode> {
    if let Commands::MQTT {
        mqtt_uri: _,
        user: _,
        token: _,
        topic,
        qos: _,
    } = &state.args.cmd
    {
        let bodydata = body.collect().await.unwrap().to_bytes();
        debug!("{:?}", bodydata);
        let cli = &state.mqtt.clone().unwrap();
        let msg: Message = Message::new(topic, bodydata, 0);
        let _ = cli.publish(msg).await;
    }

    Ok(Default::default())
}

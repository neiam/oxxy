use axum::body::Body;
use axum::extract::Request;
use axum::response::IntoResponse;
use clap::Command;
use clap::Parser;
use clap_derive::Subcommand;
use futures_lite::stream::StreamExt;
use http::{Method, StatusCode};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use lapin::options::{BasicPublishOptions, ExchangeDeclareOptions};
use lapin::types::FieldTable;
use lapin::{BasicProperties, Connection, ConnectionProperties, ExchangeKind};
use log::{debug, error, info, warn};
use oxxy::shapes::{Client, LogMessage};
use paho_mqtt as mqtt;
use paho_mqtt::Message;
use std::error::Error;
use std::io::Read;
use std::sync::Arc;
use std::thread;
use std::thread::sleep;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(
        short,
        long,
        default_value = "http://loki-loki-gateway:80/loki/api/v1/push"
    )]
    loki_uri: String,
    #[clap(short, long)]
    mqtt_uri: String,
    #[arg(short, long)]
    user: Option<String>,
    #[arg(short, long)]
    token: Option<String>,
    #[arg(long, default_value = "#/#/#/logs")]
    topic: String,
    #[arg(short, long, default_value = "0")]
    qos: i32,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();
    env_logger::init();

    let create_opts = mqtt::CreateOptionsBuilder::new()
        .server_uri(&args.mqtt_uri)
        .client_id("oxxy-moxxy") // Set a client ID for your connection
        .finalize();
    let mut cli = mqtt::AsyncClient::new(create_opts)?;

    let conn_opts = match (&args.user, &args.token) {
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
        error!("Unable to connect: {:?}", e);
        return Ok(());
    }

    info!("Moxxy Connected");

    if let Err(e) = cli.subscribe(&args.topic, args.qos).await {
        error!("Error subscribing to topic: {:?}", e);
        return Ok(());
    }
    info!("Subscribing to topic {:?}", args.topic);

    let (tx, mut rx) = mpsc::channel(100);
    let tx = Arc::new(tx); // Arc for sharing the sender across threads

    let rt_handle = tokio::runtime::Handle::current();

    cli.set_message_callback(move |cli, msg| {
        let tx = Arc::clone(&tx);
        if let Some(msg) = msg {
            // let topic = msg.topic().to_string();
            let payload = msg.payload().to_vec();
            // Spawn a new async task to send the message to the channel
            rt_handle.spawn(async move {
                if let Err(e) = tx.send(payload).await {
                    eprintln!("Error sending message: {:?}", e);
                }
            });
            // debug!("{} - {:?}", topic, payload);
        }
    });
    tokio::spawn(async move {
        while let Some(payload) = rx.recv().await {
            println!("Received message:");
            println!("Message: {:?}", &payload);
            let contenttype = match serde_json::from_slice::<LogMessage>(&payload) {
                Ok(log_message) => {
                    debug!("Successfully deserialized log message: {:?}", log_message);
                    "application/json"
                }
                Err(e) => {
                    debug!("Failed to deserialize log message: {}", e);
                    "application/x-protobuf"
                }
            };
            let payload_ = payload.to_vec();
            // Perform async operations here if needed
            // e.g., save to a database, make an HTTP request, etc.
            let client: Client =
                hyper_util::client::legacy::Client::<(), ()>::builder(TokioExecutor::new())
                    .build(HttpConnector::new());
            let req = Request::builder()
                .header("content-type", contenttype)
                .header("user-agent", "oxxy-moxxy")
                .method(Method::POST)
                .uri(args.loki_uri.as_str())
                .body(Body::from(payload_))
                .expect("Request builder error");
            let resp = client
                .request(req.into())
                .await
                .map_err(|_| StatusCode::BAD_REQUEST)
                .expect("bad req")
                .into_response();
        }
    });
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(())
}

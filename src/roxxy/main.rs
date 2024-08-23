use axum::{
    body::Body,
    extract::Request,
    response::IntoResponse,
};
use clap::Parser;

use http::Method;
use hyper::StatusCode;
use hyper_util::{client::legacy::connect::HttpConnector, rt::TokioExecutor};
use lapin::options::{ExchangeDeclareOptions, QueueBindOptions};
use lapin::{
    message::DeliveryResult,
    options::{BasicAckOptions, BasicConsumeOptions, QueueDeclareOptions},
    types::FieldTable, Connection, ConnectionProperties, ExchangeKind,
};
use log::debug;
use oxxy::shapes::{Client, LogMessage};

use std::sync::Arc;

#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(
        short,
        long,
        default_value = "http://loki-loki-gateway:80/loki/api/v1/push"
    )]
    loki_url: String,

    #[arg(short, long)]
    rmq_uri: String,

    #[arg(short, long)]
    queue: String,

    #[arg(long)]
    routing_key: String,

    #[arg(short, long)]
    exchange: String,

    #[arg(short, long, default_value = "false")]
    strict: bool,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();
    env_logger::init();

    let options = ConnectionProperties::default()
        // Use tokio executor and reactor.
        // At the moment the reactor is only available for unix.
        // .with_executor(tokio_executor_trait::Tokio::current())
        // .with_reactor(tokio_reactor_trait::Tokio)
        ;

    let connection = Connection::connect(&args.rmq_uri, options).await.unwrap();
    let channel = connection.create_channel().await.unwrap();

    channel
        .exchange_declare(
            &args.exchange,                    // exchange name
            ExchangeKind::Direct,              // type of exchange
            ExchangeDeclareOptions::default(), // default options
            FieldTable::default(),             // no extra parameters
        )
        .await?;

    let _queue = channel
        .queue_declare(
            &args.queue,
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;

    channel
        .queue_bind(
            &args.queue,       // queue name
            &args.exchange,    // exchange name
            &args.routing_key, // routing key
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let consumer = channel
        .basic_consume(
            &args.queue,
            &args.routing_key,
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let url_for_closure = Arc::new(args.loki_url.clone());

    consumer.set_delegate(move |delivery: DeliveryResult| {
        let loki_url_ = Arc::clone(&url_for_closure);
        async move {
            let delivery = match delivery {
                // Carries the delivery alongside its channel
                Ok(Some(delivery)) => delivery,
                // The consumer got canceled
                Ok(None) => return,
                // Carries the error and is always followed by Ok(None)
                Err(error) => {
                    dbg!("Failed to consume queue message {}", error);
                    return;
                }
            };

            // info!("got message {:?}", &delivery);
            let payload = &delivery.data;
            if let Ok(message) = String::from_utf8(payload.clone()) {
                let messagey = match serde_json::from_str::<LogMessage>(&message) {
                    Ok(log_message) => {
                        debug!("Successfully deserialized log message: {:?}", log_message);
                        true
                    }
                    Err(e) => {
                        debug!("Failed to deserialize log message: {}", e);
                        false
                    }
                };
                if (messagey && args.strict) || !args.strict {
                    let client: Client =
                        hyper_util::client::legacy::Client::<(), ()>::builder(TokioExecutor::new())
                            .build(HttpConnector::new());
                    let req = Request::builder()
                        .header("content-type", "application/json")
                        .header("user-agent", "oxxy-roxxy")
                        .method(Method::POST)
                        .uri(loki_url_.as_str())
                        .body(Body::from(message))
                        .expect("Request builder error");
                    let _resp = client
                        .request(req.into())
                        .await
                        .map_err(|_| StatusCode::BAD_REQUEST)
                        .expect("bad req")
                        .into_response();

                    // warn!("{:?}", body.collect().await.expect("b").to_bytes());
                    //forward
                };
                return;

                // info!("Received message: {}", message);
            } else {
                debug!(
                    "Received non-UTF-8 message, likely protobuf, passing along raw: {:?}",
                    &payload
                );
                // let s = String::from_utf8_lossy(&payload).to_string();
                // info!("{:?}", s);
                let client: Client =
                    hyper_util::client::legacy::Client::<(), ()>::builder(TokioExecutor::new())
                        .build(HttpConnector::new());
                let payload_ = payload.clone();
                let req = Request::builder()
                    .header("Content-Type", "application/x-protobuf")
                    .header("user-agent", "oxxy-roxxy")
                    .method(Method::POST)
                    .uri(loki_url_.as_str())
                    .body(Body::from(payload_))
                    .expect("Request builder error");
                let _resp = client
                    .request(req.into())
                    .await
                    .map_err(|_| StatusCode::BAD_REQUEST)
                    .expect("bad req")
                    .into_response();
            }

            // Do something with the delivery data (The message payload)

            let _ = &delivery
                .ack(BasicAckOptions::default())
                .await
                .expect("Failed to ack send_webhook_event message");
        }
    });

    std::future::pending::<()>().await;
    Ok(())
}

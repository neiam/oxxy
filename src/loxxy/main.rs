use axum::{
    body::Body,
    extract::{Request, State},
    http::uri::Uri,
    response::{IntoResponse, Response},
    routing::post,
    Router as AxumRouter,
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
use std::sync::{Arc, Mutex};
use anyhow::Context;
use tracing::{debug, info};

#[cfg(feature = "iroh-support")]
use iroh::Endpoint;
use iroh::protocol::Router;
use iroh::{NodeAddr, SecretKey};
use iroh::endpoint::SendStream;
#[cfg(feature = "iroh-support")]
use iroh_blobs::net_protocol::Blobs;
use oxxy::EXAMPLE_ALPN;
// #[cfg(feature = "iroh-support")]



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
    #[cfg(feature = "iroh-support")]
    IROH {
        #[arg(short, long)]
        node_id: iroh::NodeId,
        // #[arg(short, long)]
        // topic: String,
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
    #[cfg(feature = "iroh-support")]
    iroh: Option<Arc<Mutex<SendStream>>>,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
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
        #[cfg(feature = "iroh-support")]
        iroh: None,
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
        #[cfg(feature = "iroh-support")]
        Commands::IROH { node_id: node_id } => {
            let secret_key = SecretKey::generate(rand::rngs::OsRng);
            println!("public key: {}", secret_key.public());

            let endpoint = Endpoint::builder()
                .secret_key(secret_key)
                .alpns(vec![EXAMPLE_ALPN.to_vec()])
                .discovery_local_network().discovery_n0().bind().await?;

            let me = endpoint.node_id();
            println!("node id: {me}");
            println!("node listening addresses:");
            for local_endpoint in endpoint
                .direct_addresses()
                .initialized()
                .await
                .context("no direct addresses")?
            {
                println!("\t{}", local_endpoint.addr)
            }


            let addr = NodeAddr::new(*node_id);

            // Attempt to connect, over the given ALPN.
            // Returns a Quinn connection.
            let conn = endpoint.connect(addr, EXAMPLE_ALPN).await?;
            println!("connected");

            // Use the Quinn API to send and recv content.
            let (mut send, mut recv) = conn.open_bi().await?;

            let message = format!("{me} is saying 'hello!'");
            send.write_all(message.as_bytes()).await?;

            // Call `finish` to close the send side of the connection gracefully.
            send.finish()?;
            let message = recv.read_to_end(100).await?;
            let message = String::from_utf8(message)?;
            println!("received: {message}");

            state.iroh = Some(Arc::new(Mutex::new(send)))

            // We received the last message: close all connections and allow for the close
            // message to be sent.
            // endpoint.close().await;




            // let endpoint = Endpoint::builder().discovery_n0().bind().await?;
            // // We initialize the Blobs protocol in-memory
            // let blobs = Blobs::memory().build(&endpoint);
            //
            // // Now we build a router that accepts blobs connections & routes them
            // // to the blobs protocol.
            // let router = Router::builder(endpoint.clone())
            //     .accept(iroh_blobs::ALPN, blobs.clone())
            //     .spawn();
            // info!("iroh client initialized");
            // state.iroh = Some(endpoint);
        }
    }

    let app = match &args.cmd {
        Commands::HTTP { .. } => AxumRouter::new()
            .route("/*0", post(handler_http))
            .with_state(state),
        Commands::AMQP { .. } => AxumRouter::new()
            .route("/*0", post(handler_amqp))
            .with_state(state),
        Commands::MQTT { .. } => AxumRouter::new()
            .route("/*0", post(handler_mqtt))
            .with_state(state),
        #[cfg(feature = "iroh-support")]
        Commands::IROH { .. } => AxumRouter::new()
            .route("/{*0}", post(handler_iroh))
            .with_state(state),
    };

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4000").await?;
    info!("Loxxy listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await;
    Ok(())
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

#[cfg(feature = "iroh-support")]
async fn handler_iroh(State(state): State<Statey>, body: Body) -> Result<Response, StatusCode> {
    if let Commands::IROH { node_id: _} = &state.args.cmd {
        let bodydata = body.collect().await.unwrap().to_bytes();
        debug!("Iroh handler received data: {:?}", bodydata);
        
        // Here you would implement the actual iroh functionality
        // This is a placeholder implementation
        info!("Publishing to iroh: ");

        match state.iroh {
            None => {info!("no tunnel");}
            Some(mut iroh) => {
                let mut i = iroh.lock().unwrap();
                let message = format!("ep is saying 'hello!'");
                i.write_all(message.as_bytes()).await.expect("TODO: panic message");

                // Call `finish` to close the send side of the connection gracefully.
                i.finish().expect("TODO: panic message");
                // let message = recv.read_to_end(100).await?;
                // let message = String::from_utf8(message)?;
                // println!("received: {message}");

                // state.iroh = Some(Arc::new(send))
            }
        }

        // iroh.
        
        // Example: You might want to publish the data to an iroh document or gossip topic
        // let iroh_client = &state.iroh.clone().unwrap();
        // Implement your iroh-specific logic here
    }

    Ok(Default::default())
}

use std::process;
use std::str::FromStr;
use std::time::Duration;
use clap::Parser;
use iroh::{Endpoint, SecretKey};
use iroh::endpoint::ConnectionError;
use tracing::{debug, info, warn};
use oxxy::EXAMPLE_ALPN;

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
    shared_secret: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();
    env_logger::init();
    info!("ioxxy gateway starting up...");

    let secret = match args.shared_secret {
        None => {SecretKey::generate(rand::rngs::OsRng)}
        Some(val) => {match SecretKey::from_str(val.as_str()) {
            Ok(val) => {val}
            Err(_) => {
                info!("invalid secret key passed in, exiting");
                process::exit(0x0100)
            }
        }}
    };

    let endpoint = Endpoint::builder()
        .secret_key(secret)
        .alpns(vec![EXAMPLE_ALPN.to_vec()])
        .discovery_local_network().discovery_n0().bind().await?;

    // print this endpoint's node id
    let me = endpoint.node_id();
    println!("node id: {:?}", endpoint.node_id());

    println!("node listening addresses:");
    let local_addrs = endpoint
        .direct_addresses()
        .initialized()
        .await?
        .into_iter()
        .map(|addr| {
            let addr = addr.addr.to_string();
            println!("\t{addr}");
            addr
        })
        .collect::<Vec<_>>()
        .join(" ");
    let relay_url = endpoint.home_relay().initialized().await?;
    println!("node relay server url: {relay_url}");
    println!("\nin a separate terminal run:");
    println!(
        "\tcargo run --example connect -- --node-id {me} --addrs \"{local_addrs}\" --relay-url {relay_url}\n"
    );

    // accept incoming connections, returns a normal QUIC connection
    while let Some(incoming) = endpoint.accept().await {
        let mut connecting = match incoming.accept() {
            Ok(connecting) => connecting,
            Err(err) => {
                warn!("incoming connection failed: {err:#}");
                // we can carry on in these cases:
                // this can be caused by retransmitted datagrams
                continue;
            }
        };
        let alpn = connecting.alpn().await?;
            let conn = connecting.await?;
        let node_id = conn.remote_node_id()?;
        info!(
            "new connection from {node_id} with ALPN {}",
            String::from_utf8_lossy(&alpn),
        );

        // spawn a task to handle reading and writing off of the connection
        tokio::spawn(async move {
            // accept a bi-directional QUIC connection
            // use the `quinn` APIs to send and recv content
            let (mut send, mut recv) = conn.accept_bi().await?;
            debug!("accepted bi stream, waiting for data...");
            let message = recv.read_to_end(100).await?;
            let message = String::from_utf8(message)?;
            println!("received: {message}");

            let message = format!("hi! you connected to {me}. bye bye");
            send.write_all(message.as_bytes()).await?;
            // call `finish` to close the connection gracefully
            send.finish()?;

            // We sent the last message, so wait for the client to close the connection once
            // it received this message.
            let res = tokio::time::timeout(Duration::from_secs(3), async move {
                let closed = conn.closed().await;
                if !matches!(closed, ConnectionError::ApplicationClosed(_)) {
                    println!("node {node_id} disconnected with an error: {closed:#}");
                }
            })
                .await;
            if res.is_err() {
                println!("node {node_id} did not disconnect within 3 seconds");
            }
            Ok::<_, anyhow::Error>(())
        });
    }
    // stop with SIGINT (ctrl-c)

    Ok(())
}
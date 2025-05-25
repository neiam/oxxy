use clap::Parser;
use clap_derive::Subcommand;
use lapin::options::{BasicPublishOptions, ExchangeDeclareOptions};
use lapin::types::FieldTable;
use lapin::{BasicProperties, Channel, Connection, ConnectionProperties, ExchangeKind};
use log::info;
use paho_mqtt as mqtt;
use paho_mqtt::Message;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LOGDATAJ: &str = r#"{
  "streams": [
    {
      "stream": {
        "hostname": "zoomer",
        "hw_id": "f4:5c:89:c2:a6:07",
        "sw_version": "v2.3.5",
        "level": "info"
      },
      "values": [
        [
          "$TS$",
          "2024-08-22T13:00:00Z [INFO] - collected data"
        ],
        [
          "$TS$",
          "2024-08-22T13:00:00Z [INFO] - conditions nominal"
        ]
      ]
    }
  ]
}"#;

// const LOGDATAM: &str = r#"\x80\x05\xf0\xc2\n\xfd\x04\nG{hostname="boomer", job="systemd-journal", unit="rtkit-daemon.service"}\x12@\n\x0c\x08\xe1\xfe\x9d\xb6\x06\x10\xe0\xd9\x83\xb4\x01\x120Supervising 4 threads of 4 processes of 1 users.\x12@\n\x0c\x08\xe1\xfe\x9d\xb6\x06\x10\xf0\x97\xb3\xb4\x01\x120Supervising 4 threads of 4 processe\x05Q\x001FB\x00\x10\xd8\xd4\x85\xf2\x02\xf2\x84\x00\x08\x80\xdb\xc4\xfaB\x00\x0c\x88\xe7\xbb\xf8\xf6\x84\x00\x0c\xd8\x82\xb5\xf9\xceB\x00\x04d\n1J\xa8\xc8\x9c\x83\x8c\x03\x12TSuccessfully made thread 464465 of p)\x94\xa8 463880 owned by \'1000\' RT at priority 10.\x129\xf2\x10\xa8\xf6\xae\x8c\x036n\x01\x005\rb%\xa1\x005\x11^4es of 1 users."#;

#[derive(Clone, Debug, Subcommand)]
enum Commands {
    Mqtt {
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
    Amqp {
        #[clap(short, long)]
        rmq_uri: String,

        #[clap(short, long, default_value = "logging")]
        exchange: String,

        #[clap(short, long, default_value = "logs")]
        queue: String,

        #[clap(long, default_value = "logs")]
        routing_key: String,
    },
}

#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = "Toxxy the Oxxy Tester")]
struct Args {
    #[command(subcommand)]
    cmd: Commands,

    #[arg(short, long)]
    freq: usize,
}

pub struct Statey {
    amqp: Option<Channel>,
    mqtt: Option<mqtt::AsyncClient>,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();
    env_logger::init();
    let mut statey = Statey {
        amqp: None,
        mqtt: None,
    };
    match &args.cmd {
        Commands::Amqp { rmq_uri, .. } => {
            let options = ConnectionProperties::default();
            let connection = Connection::connect(rmq_uri, options).await.unwrap();
            let channel = connection.create_channel().await.unwrap();
            statey.amqp = Some(channel);
        }
        Commands::Mqtt {
            mqtt_uri,
            user,
            token,
            ..
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
            statey.mqtt = Some(cli);
        }
    }

    loop {
        info!("Publishing test message w/ {:?}", &args.cmd);
        match &args.cmd {
            Commands::Amqp {
                rmq_uri: _,
                queue,
                routing_key: _tag,
                exchange,
            } => {
                let channel = &statey.amqp.clone().unwrap();

                let now = SystemTime::now();
                channel
                    .exchange_declare(
                        exchange,                          // exchange name
                        ExchangeKind::Direct,              // type of exchange
                        ExchangeDeclareOptions::default(), // default options
                        FieldTable::default(),             // no extra parameters
                    )
                    .await?;
                channel
                    .basic_publish(
                        exchange,
                        queue,
                        BasicPublishOptions::default(),
                        LOGDATAJ
                            .replace(
                                "$TS$",
                                now.duration_since(UNIX_EPOCH)?
                                    .as_nanos()
                                    .to_string()
                                    .as_str(),
                            )
                            .as_ref(),
                        BasicProperties::default(),
                    )
                    .await
                    .unwrap()
                    .await
                    .unwrap();
            }
            Commands::Mqtt {
                mqtt_uri: _,
                user: _,
                token: _,
                topic,
                qos: _,
            } => {
                let cli = &statey.mqtt.clone().unwrap();
                let now = SystemTime::now();
                let ts = &now.duration_since(UNIX_EPOCH)?.as_nanos().to_string();
                let payload = LOGDATAJ.replace("$TS$", ts);
                let msg: Message = Message::new(topic, payload, 0);
                cli.publish(msg).await?;
            }
        }
        info!("{}", args.freq);
        tokio::time::sleep(Duration::new(args.freq as u64, 0)).await;
    }
}

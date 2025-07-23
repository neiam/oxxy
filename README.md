# *OXXY

A family of tools to move and proxy log data over various transports with the eventual goal of putting them into Loki

- Loxxy, A loki HTTP proxy that can
  - only deal with authentication
  - publish log data over rabbitmq / mqtt
- Moxxy and Roxxy, A pair of tools to pull data out of rabbitmq queues and mqtt topics
- Toxxy, a test data provider for publishing test logs to mqtt and rabbitmq
- Ioxxy, an Iroh Coordinator for receiving and forwarding logs

## Some Configurations

- Loxxy in your cloud, authenticating logs
- Loxxy on your edge, pushing logs into mqtt topics, Moxxy in your cloud, pushing them to loki
- Loxxy on your edge, pushing logs into mqtt (backed by rabbitmq), Roxxy in your cloud, pushing them to loki

## Inspired By

- https://github.com/k8spin/loki-multi-tenant-proxy

## LOXXY

An Authenticated Loki Proxy with configurable transport backends

To-Support:

- Injecting Org ID Header from env/cmd (opt)
- Injecting arbitrary labels from env/cmd (opt)

Backends:

- Basic HTTP Proxy
- MQTT Publisher
- AMQP Publisher
- Iroh

## MOXXY

Grabs mqtt messages and passes them to Loki

## ROXXY

Grabs RabbitMQ messages from a queue and passes them to loki

## TOXXY

a tester program that publishes either json or protobuf messages across various busses

# Mostly Kidding 👇

## DOXXY

A userauth database

## BOXXY

A Rust logger facade that ships mqtt messages



### IOXXY

Your logs, shipped over p2p

- We have a coordinator that spawns persistent pubkeys to pass into clients, clients spawn Loxxy in Iroh Mode
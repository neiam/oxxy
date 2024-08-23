use axum::body::Body;
use hyper_util::client::legacy::connect::HttpConnector;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type Client = hyper_util::client::legacy::Client<HttpConnector, Body>;
type Values = Vec<(String, String)>;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogMessage {
    stream: HashMap<String, String>,
    values: Values,
}

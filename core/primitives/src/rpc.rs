use serde::{Deserialize, Serialize};
use crate::serialize::base64_format;

#[derive(Serialize, Deserialize, Debug)]
pub struct ABCIQueryResponse {
    pub code: u32,
    pub log: String,
    pub info: String,
    pub index: i64,
    #[serde(with = "base64_format")]
    pub key: Vec<u8>,
    #[serde(with = "base64_format")]
    pub value: Vec<u8>,
    pub proof: Vec<ProofOp>,
    pub height: i64,
    pub codespace: String,
}

impl ABCIQueryResponse {
    pub fn account<T: serde::Serialize>(key: &str, value: T) -> Self {
        ABCIQueryResponse {
            code: 0,
            log: "exists".to_string(),
            info: "".to_string(),
            index: -1,
            key: key.as_bytes().to_vec(),
            value: serde_json::to_string(&value).unwrap().as_bytes().to_vec(),
            proof: vec![],
            height: 0,
            codespace: "".to_string(),
        }
    }

    pub fn result(key: &str, value: Vec<u8>, logs: Vec<String>) -> Self {
        ABCIQueryResponse {
            code: 0,
            log: logs.join("\n"),
            info: "".to_string(),
            index: -1,
            key: key.as_bytes().to_vec(),
            value,
            proof: vec![],
            height: 0,
            codespace: "".to_string(),
        }
    }

    pub fn result_err(key: &str, message: String, logs: Vec<String>) -> Self {
        ABCIQueryResponse {
            code: 1,
            log: logs.join("\n"),
            info: message,
            index: -1,
            key: key.as_bytes().to_vec(),
            value: vec![],
            proof: vec![],
            height: 0,
            codespace: "".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ProofOp {
    pub field_type: String,
    pub key: Vec<u8>,
    pub data: Vec<u8>,
}

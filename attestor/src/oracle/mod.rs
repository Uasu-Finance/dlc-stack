use secp256k1_zkp::PublicKey;
use secp256k1_zkp::{All, KeyPair, Secp256k1, SecretKey};
use serde::{Deserialize, Serialize};

mod error;
mod handler;
use crate::oracle::handler::EventHandler;
pub use error::OracleError;
pub use error::Result;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DbValue(
    pub Option<Vec<SecretKey>>,           // outstanding_sk_nonces?
    pub Vec<u8>,                          // announcement
    pub Option<Vec<u8>>,                  // attestation?
    pub Option<u64>,                      // outcome?
    pub String,                           // uuid
    #[serde(default)] pub Option<String>, // chain name
);

#[derive(Clone)]
pub struct Oracle {
    pub event_handler: EventHandler,
    pub key_pair: KeyPair,
    pub secp: Secp256k1<All>,
}

impl Oracle {
    pub fn new(
        key_pair: KeyPair,
        secp: Secp256k1<All>,
        storage_api_endpoint: String,
    ) -> Result<Oracle> {
        let event_handler = EventHandler::new(
            storage_api_endpoint,
            PublicKey::from_keypair(&key_pair).to_string(),
        );

        Ok(Oracle {
            event_handler,
            key_pair,
            secp,
        })
    }

    pub fn get_keypair(&self) -> &KeyPair {
        &self.key_pair
    }
    pub fn get_secp(&self) -> &Secp256k1<All> {
        &self.secp
    }
}

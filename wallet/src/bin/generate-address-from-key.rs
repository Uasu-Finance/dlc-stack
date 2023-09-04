use bitcoin::{Address, PrivateKey};
use std::env;

use secp256k1_zkp::{Secp256k1, SecretKey};
use serde_json::json;

fn main() {
    // Setup Blockchain Connection Object
    let network = match env::var("BITCOIN_NETWORK").as_deref() {
        Ok("bitcoin") => bitcoin::Network::Bitcoin,
        Ok("testnet") => bitcoin::Network::Testnet,
        Ok("signet") => bitcoin::Network::Signet,
        Ok("regtest") => bitcoin::Network::Regtest,
        _ => panic!(
            "Unknown Bitcoin Network, make sure to set BITCOIN_NETWORK in your env variables"
        ),
    };

    let pkey = env::var("PKEY").expect("PKEY env variable not set");
    let secp = Secp256k1::new();

    let seckey = SecretKey::from_slice(&hex::decode(pkey).unwrap()).unwrap();

    println!("Secret Key: {:?}", seckey);
    let pubkey = bitcoin::PublicKey::from_private_key(&secp, &PrivateKey::new(seckey, network));

    println!(" Pubkey: {}", pubkey);

    let pubkey = seckey.public_key(&secp);
    println!("another Pubkey: {}", pubkey);

    let bitcoin_pubkey = bitcoin::PublicKey {
        compressed: true,
        inner: pubkey,
    };

    let address = Address::p2wpkh(&bitcoin_pubkey, network).unwrap();
    println!(
        "{}",
        json!({ "public_key": pubkey, "network": network, "address": address })
    )
}

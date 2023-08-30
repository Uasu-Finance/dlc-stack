use bitcoin::Address;
use std::env;

use secp256k1_zkp::{Secp256k1, SecretKey};

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
    // seckey.keypair(&secp).

    println!("Secret Key: {:?}", seckey);
    let pubkey = seckey.public_key(&secp);
    println!("Pubkey: {}", pubkey);

    let pubkey = bitcoin::PublicKey::from_slice(&pubkey.serialize()).unwrap();

    println!("address: {}", Address::p2wpkh(&pubkey, network).unwrap());
}

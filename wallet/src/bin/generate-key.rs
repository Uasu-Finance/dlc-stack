use bdk::keys::bip39::{Language, Mnemonic, WordCount};
use bdk::keys::{DerivableKey, ExtendedKey, GeneratableKey, GeneratedKey};
use bdk::miniscript::Segwitv0;
use std::env;

use secp256k1_zkp::Secp256k1;

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

    let secp = Secp256k1::new();
    let mnemonic: GeneratedKey<_, Segwitv0> =
        Mnemonic::generate((WordCount::Words24, Language::English))
            .expect("Mnemonic generation error");
    let mnemonic = mnemonic.into_key();
    let xkey: ExtendedKey = (mnemonic.clone(), None).into_extended_key().unwrap();
    let xprv = xkey
        .into_xprv(network)
        .expect("Privatekey info not found (should not happen)");
    let fingerprint = xprv.fingerprint(&secp);
    let phrase = mnemonic
        .word_iter()
        .fold("".to_string(), |phrase, w| phrase + w + " ")
        .trim()
        .to_string();

    println!(
        "{}",
        json!({ "mnemonic": phrase, "xprv": xprv.to_string(), "fingerprint": fingerprint.to_string() })
    )
}

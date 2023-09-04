use bdk::keys::bip39::{Language, Mnemonic, WordCount};
use bdk::keys::{DerivableKey, ExtendedKey, GeneratableKey, GeneratedKey};
use bdk::miniscript::Segwitv0;
use bitcoin::util::bip32::DerivationPath;
use std::env;
use std::str::FromStr;

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

    let ext_path = DerivationPath::from_str("m/44h/0h/0h/0").expect("A valid derivation path");

    let derived_ext_xpriv = xprv.derive_priv(&secp, &ext_path).unwrap();
    let seckey_ext = derived_ext_xpriv.private_key;

    let pubkey_ext = seckey_ext.public_key(&secp);

    println!(
        "{}",
        json!({ "mnemonic": phrase, "xprv": xprv.to_string(), "fingerprint": fingerprint.to_string(), "secret_key": seckey_ext, "public_key": pubkey_ext, "network": network })
    )
}

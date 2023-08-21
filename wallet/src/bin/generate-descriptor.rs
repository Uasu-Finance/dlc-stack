use std::env;
use std::str::FromStr;

use bdk::descriptor;
use bdk::descriptor::IntoWalletDescriptor;
use bdk::keys::bip39::{Language, Mnemonic, WordCount};
use bdk::keys::{DerivableKey, GeneratableKey, GeneratedKey};
use bdk::miniscript::Segwitv0;
use bdk::Error as BDK_Error;

use secp256k1_zkp::{All, Secp256k1};

use bitcoin::util::bip32::DerivationPath;

fn main() {
    // Setup Blockchain Connection Object
    let active_network = match env::var("BITCOIN_NETWORK").as_deref() {
        Ok("bitcoin") => bitcoin::Network::Bitcoin,
        Ok("testnet") => bitcoin::Network::Testnet,
        Ok("signet") => bitcoin::Network::Signet,
        Ok("regtest") => bitcoin::Network::Regtest,
        _ => panic!(
            "Unknown Bitcoin Network, make sure to set BITCOIN_NETWORK in your env variables"
        ),
    };

    let secp: Secp256k1<All> = Secp256k1::new();

    let mnemonic: GeneratedKey<_, Segwitv0> =
        Mnemonic::generate((WordCount::Words18, Language::English))
            .map_err(|_| BDK_Error::Generic("Mnemonic generation error".to_string()))
            .unwrap();

    println!("Mnemonic phrase: {}", *mnemonic);
    let mnemonic_with_passphrase = (mnemonic.clone(), None);

    // define external and internal derivation key path
    let external_path = DerivationPath::from_str("m/86h/0h/0h/0").unwrap();
    // let internal_path = DerivationPath::from_str("m/86h/0h/0h/1").unwrap();

    // generate external and internal descriptor from mnemonic
    let (external_descriptor, ext_keymap) =
        descriptor!(wpkh((mnemonic_with_passphrase.clone(), external_path)))
            .unwrap()
            .into_wallet_descriptor(&secp, active_network)
            .unwrap();

    println!("tpub external descriptor: {}", external_descriptor);
    // println!("tpub internal descriptor: {}", internal_descriptor);
    println!(
        "tprv external descriptor: {}",
        external_descriptor.to_string_with_secret(&ext_keymap)
    );
    // println!(
    //     "tprv internal descriptor: {}",
    //     internal_descriptor.to_string_with_secret(&int_keymap)
    // );

    let xkey = mnemonic.clone().into_extended_key().unwrap();
    let xprv = xkey.into_xprv(active_network).unwrap();
    println!(
        "xprv: {:?}",
        xprv.to_priv().inner.display_secret().to_string()
    );
}

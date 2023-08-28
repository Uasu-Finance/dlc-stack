use std::{
    ops::Deref,
    sync::{Arc, Mutex},
};

use bdk::{
    database::AnyDatabase,
    sled,
    wallet::coin_selection::{BranchAndBoundCoinSelection, CoinSelectionAlgorithm},
    FeeRate,
};
use bitcoin::{hashes::Hash, Address, Network, Script};
use dlc_manager::{error::Error, Signer, Utxo, Wallet};
use secp256k1_zkp::{All, PublicKey, Secp256k1, SecretKey};

type Result<T> = core::result::Result<T, Error>;

pub struct DlcBdkWallet {
    pub bdk_wallet: Arc<Mutex<bdk::Wallet<sled::Tree>>>,
    pub address: Address,
    seckey: SecretKey,
    secp_ctx: Secp256k1<All>,
    network: Network,
}

impl DlcBdkWallet {
    /// Create a new wallet instance.
    pub fn new(
        bdk_wallet: Arc<Mutex<bdk::Wallet<sled::Tree>>>,
        address: Address,
        seckey: SecretKey,
        network: Network,
    ) -> Self {
        Self {
            bdk_wallet,
            address,
            seckey,
            secp_ctx: Secp256k1::new(),
            network,
        }
    }
}

impl Signer for DlcBdkWallet {
    fn sign_tx_input(
        &self,
        tx: &mut bitcoin::Transaction,
        input_index: usize,
        tx_out: &bitcoin::TxOut,
        _: Option<bitcoin::Script>,
    ) -> Result<()> {
        dlc::util::sign_p2wpkh_input(
            &self.secp_ctx,
            &self.seckey,
            tx,
            input_index,
            bitcoin::EcdsaSighashType::All,
            tx_out.value,
        )?;
        Ok(())
    }

    fn get_secret_key_for_pubkey(&self, _pubkey: &PublicKey) -> Result<SecretKey> {
        Ok(self.seckey)
    }
}

impl Wallet for DlcBdkWallet {
    fn get_new_address(&self) -> Result<Address> {
        Ok(self.address.clone())
    }

    fn get_new_secret_key(&self) -> Result<SecretKey> {
        Ok(self.seckey)

        /*
              let dlc_ext_path = DerivationPath::from_str("m/86h/0h/0h/2").expect("A valid derivation path");

        let mut iter = dlc_ext_path.hardened_children();
        for _ in 0..3 {
            let newpath = iter.next().unwrap();
            info!("{:?}", newpath);
            let childnums: Vec<ChildNumber> = newpath.clone().into();
            info!("{:?}", childnums);
            let derived_xpriv = xpriv.derive_priv(&secp, &newpath);

            info!("Child Number {}", derived_xpriv.unwrap().child_number);
            info!("{:?}", xpriv.derive_priv(&secp, &newpath));
        }
         */
    }

    fn get_utxos_for_amount(
        &self,
        amount: u64,
        fee_rate: Option<u64>,
        _lock_utxos: bool,
    ) -> Result<Vec<Utxo>> {
        let dummy_pubkey: PublicKey =
            "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"
                .parse()
                .unwrap();
        let dummy_drain =
            Script::new_v0_p2wpkh(&bitcoin::WPubkeyHash::hash(&dummy_pubkey.serialize()));
        let selection = BranchAndBoundCoinSelection::default()
            .coin_select(
                &AnyDatabase::Sled(self.bdk_wallet.lock().unwrap().database().deref().clone()),
                vec![],
                vec![],
                FeeRate::from_sat_per_vb(fee_rate.unwrap_or(0) as f32),
                amount,
                &dummy_drain,
            )
            .unwrap();

        let mut res = Vec::new();

        for utxo in selection.selected {
            res.push(dlc_manager::Utxo {
                outpoint: utxo.outpoint(),
                tx_out: utxo.txout().clone(),
                address: self.address.clone(),
                redeem_script: Script::new(), // What is this for, and where can I get it when using BDK to manage UTXOs?
                reserved: false,
            });
        }
        Ok(res)
    }

    fn import_address(&self, _: &Address) -> Result<()> {
        Ok(())
    }
}

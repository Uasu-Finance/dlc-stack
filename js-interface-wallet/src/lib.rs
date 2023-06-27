use std::{cell::RefCell, ops::Deref, str::FromStr};

use bdk::{
    database::{BatchOperations, Database},
    wallet::coin_selection::{
        decide_change, BranchAndBoundCoinSelection, CoinSelectionAlgorithm, CoinSelectionResult,
    },
    FeeRate, KeychainKind, LocalUtxo, Utxo as BdkUtxo, WeightedUtxo,
};
use bitcoin::{
    hashes::Hash, Address, Network, PackedLockTime, PrivateKey, Script, Sequence, Transaction,
    TxIn, TxOut, Txid, Witness,
};
use dlc_manager::{error::Error, Blockchain, Signer, Utxo, Wallet};
use lightning::chain::chaininterface::{ConfirmationTarget, FeeEstimator};
use rust_bitcoin_coin_selection::select_coins;
use secp256k1_zkp::{rand::thread_rng, All, PublicKey, Secp256k1, SecretKey};
type Result<T> = core::result::Result<T, Error>;

pub(crate) const TXIN_BASE_WEIGHT: usize = (32 + 4 + 4) * 4;

/// Trait providing blockchain information to the wallet.
pub trait WalletBlockchainProvider: Blockchain + FeeEstimator {
    fn get_utxos_for_address(&self, address: &Address) -> Result<Vec<Utxo>>;
    fn is_output_spent(&self, txid: &Txid, vout: u32) -> Result<bool>;
}

/// Trait enabling the wallet to persist data.
// pub trait WalletStorage {
//     fn upsert_address(&self, address: &Address, privkey: &SecretKey) -> Result<()>;
//     fn delete_address(&self, address: &Address) -> Result<()>;
//     fn get_addresses(&self) -> Result<Vec<Address>>;
//     fn get_priv_key_for_address(&self, address: &Address) -> Result<Option<SecretKey>>;
//     fn upsert_key_pair(&self, public_key: &PublicKey, privkey: &SecretKey) -> Result<()>;
//     fn get_priv_key_for_pubkey(&self, public_key: &PublicKey) -> Result<Option<SecretKey>>;
//     fn upsert_utxo(&self, utxo: &Utxo) -> Result<()>;
//     fn has_utxo(&self, utxo: &Utxo) -> Result<bool>;
//     fn delete_utxo(&self, utxo: &Utxo) -> Result<()>;
//     fn get_utxos(&self) -> Result<Vec<Utxo>>;
//     fn unreserve_utxo(&self, txid: &Txid, vout: u32) -> Result<()>;
// }

/// Basic wallet mainly meant for testing purposes.
// pub struct JSInterfaceWallet<B: Deref, W: Deref>
// where
//     B::Target: WalletBlockchainProvider,
//     W::Target: WalletStorage,
// {
//     blockchain: B,
//     storage: W,
//     secp_ctx: Secp256k1<All>,
//     network: Network,
// }

pub struct JSInterfaceWallet {
    address: Address,
    secp_ctx: Secp256k1<All>,
    seckey: SecretKey,
    network: Network,
    utxos: RefCell<Option<Vec<Utxo>>>,
}

impl JSInterfaceWallet {
    pub fn new(address_str: String, network: Network, privkey: PrivateKey) -> Self {
        // let secp_ctx = Secp256k1::new();

        Self {
            address: Address::from_str(&address_str).unwrap(),
            // address: Address::p2wpkh(
            //     &bitcoin::PublicKey::from_private_key(&secp_ctx, &privkey),
            //     network,
            // )
            // .unwrap(),
            secp_ctx: Secp256k1::new(),
            seckey: privkey.inner,
            network,
            utxos: Some(vec![]).into(),
        }
    }

    pub fn set_utxos(&self, mut utxos: Vec<Utxo>) -> Result<()> {
        self.utxos.borrow_mut().as_mut().unwrap().clear();
        self.utxos.borrow_mut().as_mut().unwrap().append(&mut utxos);
        Ok(())
    }

    // where
    //     B::Target: WalletBlockchainProvider,
    //     W::Target: WalletStorage,
    // {
    //     /// Create a new wallet instance.
    //     pub fn new(blockchain: B, storage: W, network: Network) -> Self {
    //         Self {
    //             blockchain,
    //             storage,
    //             secp_ctx: Secp256k1::new(),
    //             network,
    //         }
    //     }

    // Refresh the wallet checking and updating the UTXO states.
    // pub fn refresh(&self) -> Result<()> {
    //     let utxos: Vec<Utxo> = self.storage.get_utxos()?;

    //     for utxo in &utxos {
    //         let is_spent = self
    //             .blockchain
    //             .is_output_spent(&utxo.outpoint.txid, utxo.outpoint.vout)?;
    //         if is_spent {
    //             self.storage.delete_utxo(utxo)?;
    //         }
    //     }

    //     let addresses = self.storage.get_addresses()?;

    //     for address in &addresses {
    //         let utxos = self.blockchain.get_utxos_for_address(address)?;

    //         for utxo in &utxos {
    //             if !self.storage.has_utxo(utxo)? {
    //                 self.storage.upsert_utxo(utxo)?;
    //             }
    //         }
    //     }

    //     Ok(())
    // }

    // Returns the sum of all UTXOs value.
    pub fn get_balance(&self) -> u64 {
        self.utxos
            .borrow()
            .as_ref()
            .unwrap()
            .iter()
            .map(|x| x.tx_out.value)
            .sum()
    }

    // Mark all UTXOs as unreserved.
    // pub fn unreserve_all_utxos(&self) {
    //     let utxos = self.storage.get_utxos().unwrap();
    //     for utxo in utxos {
    //         self.storage
    //             .unreserve_utxo(&utxo.outpoint.txid, utxo.outpoint.vout)
    //             .unwrap();
    //     }
    // }

    // / Creates a transaction with all wallet UTXOs as inputs and a single output
    // / sending everything to the given address.
    // pub fn empty_to_address(&self, address: &Address) -> Result<()> {
    //     let utxos = self
    //         .storage
    //         .get_utxos()
    //         .expect("to be able to retrieve all utxos");
    //     if utxos.is_empty() {
    //         return Err(Error::InvalidState("No utxo in wallet".to_string()));
    //     }

    //     let mut total_value = 0;
    //     let input = utxos
    //         .iter()
    //         .map(|x| {
    //             total_value += x.tx_out.value;
    //             TxIn {
    //                 previous_output: x.outpoint,
    //                 script_sig: Script::default(),
    //                 sequence: Sequence::MAX,
    //                 witness: Witness::default(),
    //             }
    //         })
    //         .collect::<Vec<_>>();
    //     let output = vec![TxOut {
    //         value: total_value,
    //         script_pubkey: address.script_pubkey(),
    //     }];
    //     let mut tx = Transaction {
    //         version: 2,
    //         lock_time: PackedLockTime::ZERO,
    //         input,
    //         output,
    //     };
    //     // Signature + pubkey size assuming P2WPKH.
    //     let weight = (tx.weight() + tx.input.len() * (74 + 33)) as u64;
    //     let fee_rate = self
    //         .blockchain
    //         .get_est_sat_per_1000_weight(ConfirmationTarget::Normal) as u64;
    //     let fee = (weight * fee_rate) / 1000;
    //     tx.output[0].value -= fee;

    //     for (i, utxo) in utxos.iter().enumerate().take(tx.input.len()) {
    //         self.sign_tx_input(&mut tx, i, &utxo.tx_out, None)?;
    //     }

    //     self.blockchain.send_transaction(&tx)
    // }
}

impl Signer for JSInterfaceWallet {
    fn sign_tx_input(
        &self,
        tx: &mut bitcoin::Transaction,
        input_index: usize,
        tx_out: &bitcoin::TxOut,
        _: Option<bitcoin::Script>,
    ) -> Result<()> {
        // let address = Address::from_script(&tx_out.script_pubkey, self.network)
        //     .expect("a valid scriptpubkey");
        // let seckey = self
        //     .storage
        //     .get_priv_key_for_address(&address)?
        //     .expect("to have the requested private key");
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

    fn get_secret_key_for_pubkey(&self, pubkey: &PublicKey) -> Result<SecretKey> {
        Ok(self.seckey)
    }
}

fn select_sorted_utxos(
    utxos: impl Iterator<Item = (bool, WeightedUtxo)>,
    fee_rate: FeeRate,
    target_amount: u64,
    drain_script: &Script,
) -> Result<CoinSelectionResult> {
    let mut selected_amount = 0;
    let mut fee_amount = 0;
    let selected = utxos
        .scan(
            (&mut selected_amount, &mut fee_amount),
            |(selected_amount, fee_amount), (must_use, weighted_utxo)| {
                if must_use || **selected_amount < target_amount + **fee_amount {
                    **fee_amount +=
                        fee_rate.fee_wu(TXIN_BASE_WEIGHT + weighted_utxo.satisfaction_weight);
                    **selected_amount += weighted_utxo.utxo.txout().value;

                    Some(weighted_utxo.utxo)
                } else {
                    None
                }
            },
        )
        .collect::<Vec<_>>();

    let amount_needed_with_fees = target_amount + fee_amount;
    if selected_amount < amount_needed_with_fees {
        panic!("insufficient funds");
    }

    let remaining_amount = selected_amount - amount_needed_with_fees;

    let excess = decide_change(remaining_amount, fee_rate, drain_script);

    Ok(CoinSelectionResult {
        selected,
        fee_amount,
        excess,
    })
}

impl Wallet for JSInterfaceWallet {
    fn get_new_address(&self) -> Result<Address> {
        Ok(self.address.clone())
    }

    fn get_new_secret_key(&self) -> Result<SecretKey> {
        Ok(self.seckey)
    }

    fn get_utxos_for_amount(
        &self,
        amount: u64,
        fee_rate: Option<u64>,
        lock_utxos: bool,
    ) -> Result<Vec<Utxo>> {
        let org_utxos = self.utxos.borrow().as_ref().unwrap().clone();
        let mut utxos = org_utxos
            .iter()
            .filter(|x| !x.reserved)
            .map(|x| WeightedUtxo {
                utxo: BdkUtxo::Local(LocalUtxo {
                    outpoint: x.outpoint,
                    txout: x.tx_out.clone(),
                    keychain: KeychainKind::External,
                    is_spent: false,
                }),
                satisfaction_weight: 107,
            })
            .collect::<Vec<_>>();
        let dummy_pubkey: PublicKey =
            "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"
                .parse()
                .unwrap();
        let dummy_drain =
            Script::new_v0_p2wpkh(&bitcoin::WPubkeyHash::hash(&dummy_pubkey.serialize()));
        let fee_rate = FeeRate::from_sat_per_vb(fee_rate.unwrap() as f32);
        let required_utxos = Vec::new();
        let drain_script = &dummy_drain;

        let temp_utxos = {
            utxos.sort_unstable_by_key(|wu| wu.utxo.txout().value);
            required_utxos
                .into_iter()
                .map(|utxo| (true, utxo))
                .chain(utxos.into_iter().rev().map(|utxo| (false, utxo)))
        };

        let selection = select_sorted_utxos(temp_utxos, fee_rate, amount, drain_script).unwrap();

        let mut res = Vec::new();
        for utxo in selection.selected {
            let local_utxo = if let BdkUtxo::Local(l) = utxo {
                l
            } else {
                panic!();
            };
            let org = org_utxos
                .iter()
                .find(|x| x.tx_out == local_utxo.txout && x.outpoint == local_utxo.outpoint)
                .unwrap();
            res.push(org.clone());
        }
        Ok(res)
    }

    fn import_address(&self, _: &Address) -> Result<()> {
        // unimplemented!()
        Ok(())
    }
}

#[derive(Clone)]
struct UtxoWrap {
    utxo: Utxo,
}

impl rust_bitcoin_coin_selection::Utxo for UtxoWrap {
    fn get_value(&self) -> u64 {
        self.utxo.tx_out.value
    }
}

// #[cfg(test)]
// mod tests {
//     use std::rc::Rc;

//     use dlc_manager::{Signer, Wallet};
//     use mocks::simple_wallet::JSInterfaceWallet;
//     use mocks::{memory_storage_provider::MemoryStorage, mock_blockchain::MockBlockchain};
//     use secp256k1_zkp::{PublicKey, SECP256K1};

//     fn get_wallet() -> JSInterfaceWallet<Rc<MockBlockchain>, Rc<MemoryStorage>> {
//         let blockchain = Rc::new(MockBlockchain {});
//         let storage = Rc::new(MemoryStorage::new());
//         let wallet = JSInterfaceWallet::new(blockchain, storage, bitcoin::Network::Regtest);
//         wallet
//     }

//     #[test]
//     fn get_new_secret_key_can_be_retrieved() {
//         let wallet = get_wallet();
//         let sk = wallet.get_new_secret_key().unwrap();
//         let pk = PublicKey::from_secret_key(SECP256K1, &sk);

//         let sk2 = wallet.get_secret_key_for_pubkey(&pk).unwrap();

//         assert_eq!(sk, sk2);
//     }
// }

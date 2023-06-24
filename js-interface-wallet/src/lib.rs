use std::vec;
use std::{cell::RefCell, ops::Deref, str::FromStr};

use bdk::blockchain::{GetHeight, WalletSync};
use bdk::TransactionDetails;

use bdk::esplora_client::{AsyncClient, Builder};
use bdk::{database::MemoryDatabase, template::Bip84, SyncOptions, Wallet};
use bdk::{
    wallet::coin_selection::{BranchAndBoundCoinSelection, CoinSelectionAlgorithm},
    FeeRate, KeychainKind, Utxo as BdkUtxo,
};

use bitcoin::util::bip32::ExtendedPrivKey;
use bitcoin::OutPoint;
use bitcoin::{hashes::Hash, Address, Network, PrivateKey, Script, TxOut, Txid};
use dlc_manager::{error::Error, Blockchain as DLCBlockchain, Signer, Utxo, Wallet as DLCWallet};
use lightning::chain::chaininterface::FeeEstimator;

use secp256k1_zkp::{All, PublicKey, Secp256k1, SecretKey};
use serde::{Deserialize, Serialize};

type Result<T> = core::result::Result<T, Error>;

/// Trait providing blockchain information to the wallet.
pub trait WalletBlockchainProvider: DLCBlockchain + FeeEstimator {
    fn get_utxos_for_address(&self, address: &Address) -> Result<Vec<Utxo>>;
    fn is_output_spent(&self, txid: &Txid, vout: u32) -> Result<bool>;
}

/// Trait enabling the wallet to persist data.
pub trait WalletStorage {
    fn upsert_address(&self, address: &Address, privkey: &SecretKey) -> Result<()>;
    fn delete_address(&self, address: &Address) -> Result<()>;
    fn get_addresses(&self) -> Result<Vec<Address>>;
    fn get_priv_key_for_address(&self, address: &Address) -> Result<Option<SecretKey>>;
    fn upsert_key_pair(&self, public_key: &PublicKey, privkey: &SecretKey) -> Result<()>;
    fn get_priv_key_for_pubkey(&self, public_key: &PublicKey) -> Result<Option<SecretKey>>;
    fn upsert_utxo(&self, utxo: &Utxo) -> Result<()>;
    fn has_utxo(&self, utxo: &Utxo) -> Result<bool>;
    fn delete_utxo(&self, utxo: &Utxo) -> Result<()>;
    fn get_utxos(&self) -> Result<Vec<Utxo>>;
    fn unreserve_utxo(&self, txid: &Txid, vout: u32) -> Result<()>;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct UTXOSpent {
    spent: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum UtxoStatus {
    Confirmed {
        confirmed: bool,
        block_height: u64,
        block_hash: String,
        block_time: u64,
    },
    Unconfirmed {
        confirmed: bool,
    },
}

#[derive(Serialize, Deserialize, Debug)]
struct UtxoResp {
    txid: String,
    vout: u32,
    value: u64,
    status: UtxoStatus,
}

pub struct JSInterfaceWallet {
    address: Address,
    client: AsyncClient,
    bdk_wallet: Wallet<MemoryDatabase>,
    height: RefCell<Option<u64>>,
    secp_ctx: Secp256k1<All>,
    seckey: SecretKey,
    utxos: RefCell<Option<Vec<Utxo>>>,
}

impl JSInterfaceWallet {
    pub fn new(address_str: String, base_url: String, privkey: String, network: Network) -> Self {
        // Even though the esplora blockchain provider has it's own client, we need one of our own to get some additional data
        let client = Builder::new(&base_url)
            .build_async()
            .expect("Should never fail with no proxy and timeout");

        // Generate keypair from secret key
        let seckey = secp256k1_zkp::SecretKey::from_str(&privkey).unwrap();

        let epkey = ExtendedPrivKey::from_str(&privkey.to_string()).unwrap();
        // let secp_ctx = Secp256k1::new();
        let bdk_wallet = Wallet::new(
            Bip84(epkey, KeychainKind::External),
            Some(Bip84(epkey, KeychainKind::Internal)),
            network,
            MemoryDatabase::default(),
        )
        .unwrap();

        Self {
            address: Address::from_str(&address_str).unwrap(),
            client,
            height: Some(0).into(),
            secp_ctx: Secp256k1::new(),
            seckey: PrivateKey::new(seckey, network).inner,
            utxos: Some(vec![]).into(),
            bdk_wallet,
        }
    }

    async fn get_from_json<T>(&self, path: &str) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        self.client
            .client()
            .get(path)
            .send()
            .await
            .unwrap()
            .json::<T>()
            .await
            .map_err(|e| Error::BlockchainError(e.to_string()))
    }

    pub async fn sync<B: WalletSync + GetHeight>(&self, blockchain: &B, address: String) -> () {
        self.bdk_wallet
            .sync(blockchain, SyncOptions::default())
            .await
            .unwrap();
        self.height
            .replace(Some(blockchain.get_height().await.unwrap().into()));
        self.set_utxos_for_address(address).await;
    }

    pub fn get_height(&self) -> u64 {
        self.height.borrow().unwrap().clone()
    }

    pub fn get_tx(&self, txid: &Txid) -> Result<Option<TransactionDetails>> {
        Ok(self.bdk_wallet.get_tx(txid, false).unwrap())
    }

    pub fn get_utxos(&self) -> Result<Vec<Utxo>> {
        Ok(self.utxos.borrow().as_ref().unwrap().clone())
    }

    pub fn network(&self) -> Network {
        self.bdk_wallet.network()
    }

    pub fn client(&self) -> &AsyncClient {
        &self.client
    }

    // gets all the utxos and txs and height of chain for one address only.
    // This does not support multiple addresses
    async fn set_utxos_for_address(&self, address: String) -> () {
        let utxos: Vec<UtxoResp> = self
            .get_from_json(&format!("address/{address}/utxo"))
            .await
            .unwrap();

        let address = Address::from_str(&address).unwrap();
        let mut utxos = utxos
            .into_iter()
            .map(|x| Utxo {
                address: address.clone(),
                outpoint: OutPoint {
                    txid: x
                        .txid
                        .parse()
                        .map_err(|e: <bitcoin::Txid as FromStr>::Err| {
                            Error::BlockchainError(e.to_string())
                        })
                        .unwrap(),
                    vout: x.vout,
                },
                redeem_script: Script::default(),
                reserved: false,
                tx_out: TxOut {
                    value: x.value,
                    script_pubkey: address.script_pubkey(),
                },
            })
            .collect::<Vec<Utxo>>();

        let mut utxo_spent_pairs = Vec::new();
        for utxo in utxos.clone() {
            let is_spent: UTXOSpent = self
                .get_from_json::<UTXOSpent>(&format!(
                    "tx/{0}/outspend/{1}",
                    &utxo.outpoint.txid, utxo.outpoint.vout
                ))
                .await
                .unwrap();
            utxo_spent_pairs.push((utxo, is_spent.spent));
        }

        self.utxos
            .try_borrow_mut()
            .unwrap() // FIXME this blows up sometimes!
            .as_mut()
            .unwrap()
            .clear();
        self.utxos.borrow_mut().as_mut().unwrap().append(&mut utxos);
    }

    // Returns the sum of all UTXOs value.
    pub fn get_balance(&self) -> u64 {
        // self.bdk_wallet.get_balance().unwrap().get_spendable()
        self.bdk_wallet.get_balance().unwrap().get_total()
    }
}

impl Signer for JSInterfaceWallet {
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

impl DLCWallet for JSInterfaceWallet {
    fn get_new_address(&self) -> Result<Address> {
        Ok(self.address.clone())
    }

    fn get_new_secret_key(&self) -> Result<SecretKey> {
        Ok(self.seckey)
    }

    // This code is copied and modified from rust-dlc simple-wallet, please reference that
    fn get_utxos_for_amount(
        &self,
        amount: u64,
        fee_rate: Option<u64>,
        _lock_utxos: bool,
    ) -> Result<Vec<Utxo>> {
        let org_utxos = self.get_utxos().unwrap();
        let coin_selection = BranchAndBoundCoinSelection::default();
        let dummy_pubkey: PublicKey =
            "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"
                .parse()
                .unwrap();
        let dummy_drain =
            Script::new_v0_p2wpkh(&bitcoin::WPubkeyHash::hash(&dummy_pubkey.serialize()));
        let fee_rate = FeeRate::from_sat_per_vb(fee_rate.unwrap() as f32);
        let selection = coin_selection
            .coin_select(
                self.bdk_wallet.database().deref(),
                Vec::new(),
                vec![], // could be "utxos" converted from org_utxos if we don't want to use the build in wallet db
                fee_rate,
                amount,
                &dummy_drain,
            )
            .map_err(|e| Error::WalletError(Box::new(e)))?;
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
        // Ask what this is for
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

#![feature(async_fn_in_trait)]
#![allow(unreachable_code)]
extern crate console_error_panic_hook;
extern crate log;

use bitcoin::{Network, PrivateKey, XOnlyPublicKey};
use dlc_link_manager::AsyncOracle;
use dlc_messages::{Message, OfferDlc, SignDlc};
use wasm_bindgen::prelude::*;

use lightning::util::ser::Readable;

use secp256k1_zkp::hashes::*;
use secp256k1_zkp::Secp256k1;

use core::panic;
use std::{
    collections::HashMap,
    io::Cursor,
    str::FromStr,
    sync::{Arc, Mutex},
};

use dlc_manager::{
    contract::{signed_contract::SignedContract, Contract},
    ContractId, SystemTimeProvider,
};

use dlc_link_manager::{AsyncStorage, Manager};

use std::fmt::Write as _;

use storage::async_storage_api::AsyncStorageApiProvider;

use esplora_async_blockchain_provider::EsploraAsyncBlockchainProvider;

use js_interface_wallet::JSInterfaceWallet;

use attestor_client::AttestorClient;
use serde::{Deserialize, Serialize};

mod storage;
#[macro_use]
mod macros;

async fn generate_attestor_client(
    attestor_urls: Vec<String>,
) -> HashMap<XOnlyPublicKey, Arc<AttestorClient>> {
    let mut attestor_clients = HashMap::new();

    for url in attestor_urls.iter() {
        let p2p_client: AttestorClient = AttestorClient::new(url).await.unwrap();
        let attestor = Arc::new(p2p_client);
        attestor_clients.insert(attestor.get_public_key().await, attestor.clone());
    }
    return attestor_clients;
}

type DlcManager = Manager<
    Arc<JSInterfaceWallet>,
    Arc<EsploraAsyncBlockchainProvider>,
    Box<AsyncStorageApiProvider>,
    Arc<AttestorClient>,
    Arc<SystemTimeProvider>,
>;

// The contracts in dlc-manager expect a node id, but web extensions often don't have this, so hardcode it for now. Should not have any ramifications.
const STATIC_COUNTERPARTY_NODE_ID: &str =
    "02fc8e97419286cf05e5d133f41ff6d51f691dda039e9dc007245a421e2c7ec61c";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ErrorResponse {
    message: String,
    code: Option<u64>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ErrorsResponse {
    errors: Vec<ErrorResponse>,
    status: u64,
}

#[derive(Serialize, Deserialize)]
struct UtxoInput {
    txid: String,
    vout: u32,
    value: u64,
}

#[wasm_bindgen]
pub struct JsDLCInterface {
    options: JsDLCInterfaceOptions,
    manager: Arc<Mutex<DlcManager>>,
    wallet: Arc<JSInterfaceWallet>,
    blockchain: Arc<EsploraAsyncBlockchainProvider>,
}

// #[wasm_bindgen]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JsDLCInterfaceOptions {
    attestor_urls: String,
    network: String,
    electrs_url: String,
    address: String,
}

impl Default for JsDLCInterfaceOptions {
    // Default values for Manager Options
    fn default() -> Self {
        Self {
            attestor_urls: "https://dev-oracle.dlc.link/oracle".to_string(),
            network: "regtest".to_string(),
            electrs_url: "https://dev-oracle.dlc.link/electrs".to_string(),
            address: "".to_string(),
        }
    }
}

#[wasm_bindgen]
impl JsDLCInterface {
    pub async fn new(
        privkey: String,
        address: String,
        network: String,
        electrs_url: String,
        attestor_urls: String,
    ) -> JsDLCInterface {
        console_error_panic_hook::set_once();

        let options = JsDLCInterfaceOptions {
            attestor_urls,
            network,
            electrs_url,
            address,
        };

        let active_network: Network = options
            .network
            .parse::<Network>()
            .expect("Must use a valid bitcoin network");

        let blockchain: Arc<EsploraAsyncBlockchainProvider> = Arc::new(
            EsploraAsyncBlockchainProvider::new(options.electrs_url.to_string(), active_network),
        );

        // Generate keypair from secret key
        let seckey = secp256k1_zkp::SecretKey::from_str(&privkey).unwrap();

        let secp = Secp256k1::new();

        // let pubkey = PublicKey::from_secret_key(&secp, &seckey);
        let pubkey =
            bitcoin::PublicKey::from_private_key(&secp, &PrivateKey::new(seckey, active_network));

        // Set up DLC store
        let dlc_store = AsyncStorageApiProvider::new(
            pubkey.to_string(),
            "https://devnet.dlc.link/storage-api".to_string(),
        );

        // Set up wallet
        let wallet = Arc::new(JSInterfaceWallet::new(
            options.address.to_string(),
            active_network,
            PrivateKey::new(seckey, active_network),
        ));

        // Set up Oracle Clients
        let attestor_urls_vec: Vec<String> =
            match serde_json::from_str(&options.attestor_urls.clone()) {
                Ok(vec) => vec,
                Err(e) => {
                    eprintln!("Error deserializing Attestor URLs: {}", e);
                    Vec::new()
                }
            };

        let attestors = generate_attestor_client(attestor_urls_vec).await;

        // Set up time provider
        let time_provider = SystemTimeProvider {};

        // Create the DLC Manager
        let manager = Arc::new(Mutex::new(
            Manager::new(
                Arc::clone(&wallet),
                Arc::clone(&blockchain),
                Box::new(dlc_store),
                attestors,
                Arc::new(time_provider),
            )
            .unwrap(),
        ));

        blockchain.refresh_chain_data(options.address.clone()).await;

        JsDLCInterface {
            options,
            manager,
            wallet,
            blockchain,
        }
    }

    pub fn get_options(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.options).unwrap()
    }

    pub async fn get_wallet_balance(&self) -> u64 {
        self.blockchain
            .refresh_chain_data(self.options.address.clone())
            .await;
        self.wallet
            .set_utxos(self.blockchain.get_utxos().unwrap())
            .unwrap();
        self.blockchain.get_balance().await.unwrap()
    }

    // public async function for fetching all the contracts on the manager
    pub async fn get_contracts(&self) -> JsValue {
        let contracts: Vec<JsContract> = self
            .manager
            .lock()
            .unwrap()
            .get_store()
            .get_contracts()
            .await
            .unwrap()
            .into_iter()
            .map(|contract| JsContract::from_contract(contract))
            .collect();

        serde_wasm_bindgen::to_value(&contracts).unwrap()
    }

    // public async function for fetching one contract as a JsContract type
    pub async fn get_contract(&self, contract_str: String) -> JsValue {
        let contract_id = ContractId::read(&mut Cursor::new(&contract_str)).unwrap();
        let contract = self
            .manager
            .lock()
            .unwrap()
            .get_store()
            .get_contract(&contract_id)
            .await
            .unwrap();
        match contract {
            Some(contract) => {
                serde_wasm_bindgen::to_value(&JsContract::from_contract(contract)).unwrap()
            }
            None => JsValue::NULL,
        }
    }

    pub async fn accept_offer(&self, offer_json: String) -> String {
        let dlc_offer_message: OfferDlc = serde_json::from_str(&offer_json).unwrap();

        let temporary_contract_id = dlc_offer_message.temporary_contract_id;

        match self
            .manager
            .lock()
            .unwrap()
            .on_dlc_message(
                &Message::Offer(dlc_offer_message.clone()),
                STATIC_COUNTERPARTY_NODE_ID.parse().unwrap(),
            )
            .await
        {
            Ok(_) => (),
            Err(e) => {
                return e.to_string();
            }
        }

        let (_contract_id, _public_key, accept_msg) = self
            .manager
            .lock()
            .unwrap()
            .accept_contract_offer(&temporary_contract_id)
            .await
            .expect("Error accepting contract offer");

        serde_json::to_string(&accept_msg).unwrap()
    }

    pub async fn countersign_and_broadcast(&self, dlc_sign_message: String) -> String {
        let dlc_sign_message: SignDlc = serde_json::from_str(&dlc_sign_message).unwrap();
        match self
            .manager
            .lock()
            .unwrap()
            .on_dlc_message(
                &Message::Sign(dlc_sign_message.clone()),
                STATIC_COUNTERPARTY_NODE_ID.parse().unwrap(),
            )
            .await
        {
            Ok(_) => (),
            Err(e) => {
                log_to_console!("DLC manager - sign offer error: {}", e.to_string());
                panic!();
            }
        }
        let manager = self.manager.lock().unwrap();
        let store = manager.get_store();
        let contract: SignedContract = store
            .get_signed_contracts()
            .await
            .unwrap()
            .into_iter()
            .filter(|c| c.accepted_contract.get_contract_id() == dlc_sign_message.contract_id)
            .next()
            .unwrap();
        contract
            .accepted_contract
            .dlc_transactions
            .fund
            .txid()
            .to_string()
    }

    pub async fn reject_offer(&self, contract_id: String) -> () {
        let contract_id = ContractId::read(&mut Cursor::new(&contract_id)).unwrap();
        let contract = self
            .manager
            .lock()
            .unwrap()
            .get_store()
            .get_contract(&contract_id)
            .await
            .unwrap();

        match contract {
            Some(Contract::Offered(c)) => {
                self.manager
                    .lock()
                    .unwrap()
                    .get_store()
                    .update_contract(&Contract::Rejected(c))
                    .await
                    .unwrap();
            }
            _ => (),
        }
    }

    // fn accept_offer(&self, offer_json: String, utxos: String) -> String {
    // self.blockchain.refresh_chain_data(address.clone()).await;
    // let utxo_inputs: Vec<UtxoInput> = serde_json::from_str(&utxos).unwrap();

    // log out utxo_inputs
    // clog!("utxo_inputs {:?}", utxo_inputs);

    // let utxos: Vec<Utxo> = utxo_inputs
    //     .into_iter()
    //     .map(|utxo| Utxo {
    //         address: address.clone(),
    //         outpoint: OutPoint {
    //             txid: utxo
    //                 .txid
    //                 .parse()
    //                 .expect("To be able to parse the txid from the utxo"),
    //             vout: utxo.vout,
    //         },
    //         redeem_script: Script::default(),
    //         reserved: false,
    //         tx_out: TxOut {
    //             value: utxo.value,
    //             script_pubkey: address.script_pubkey(),
    //         },
    //     })
    //     .collect();

    // log out utxos
    // clog!("utxos {:?}", utxos);

    // Then the utxos can be set in the js-interface-wallet as the current utxos, overwriting the previous ones
    // the wallet should use that list for the remainder of the operation.
    // self.wallet.set_utxos(utxos).unwrap();

    //     let dlc_offer_message: OfferDlc = serde_json::from_str(&offer_json).unwrap();
    //     clog!("Offer to accept: {:?}", dlc_offer_message);
    //     match self.manager.lock().unwrap().on_dlc_message(
    //         &Message::Offer(dlc_offer_message.clone()),
    //         STATIC_COUNTERPARTY_NODE_ID.parse().unwrap(),
    //     ) {
    //         Ok(_) => (),
    //         Err(e) => {
    //             clog!("DLC manager - receive offer error: {}", e.to_string());
    //             return "".to_string();
    //         }
    //     }

    //     clog!("receive_offer - after on_dlc_message");
    //     let temporary_contract_id = dlc_offer_message.temporary_contract_id;

    //     clog!("accepting contract with id {:?}", temporary_contract_id);

    //     let (_contract_id, _public_key, accept_msg) = self
    //         .manager
    //         .lock()
    //         .unwrap()
    //         .accept_contract_offer(&temporary_contract_id)
    //         .expect("Error accepting contract offer");

    //     clog!("receive_offer - after accept_contract_offer");
    //     serde_json::to_string(&accept_msg).unwrap()
    // }
}

#[derive(Serialize, Deserialize)]
#[wasm_bindgen]
struct JsContract {
    id: String,
    state: String,
}

// implement the from_contract method for JsContract
impl JsContract {
    fn from_contract(contract: Contract) -> JsContract {
        let state = match contract {
            Contract::Offered(_) => "offered",
            Contract::Accepted(_) => "accepted",
            Contract::Signed(_) => "signed",
            Contract::Confirmed(_) => "confirmed",
            Contract::PreClosed(_) => "pre-closed",
            Contract::Closed(_) => "closed",
            Contract::Refunded(_) => "refunded",
            Contract::FailedAccept(_) => "failed accept",
            Contract::FailedSign(_) => "failed sign",
            Contract::Rejected(_) => "rejected",
        };

        fn hex_str(value: &[u8]) -> String {
            let mut res = String::with_capacity(64);
            for v in value {
                write!(res, "{:02x}", v).unwrap();
            }
            res
        }

        JsContract {
            id: hex_str(&contract.get_id()),
            state: state.to_string(),
        }
    }
}

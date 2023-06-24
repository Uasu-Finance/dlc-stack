#![allow(unreachable_code)]
extern crate console_error_panic_hook;
extern crate log;

use bdk::blockchain::EsploraBlockchain;
use bitcoin::Network;
use dlc_messages::{Message, OfferDlc, SignDlc};
use wasm_bindgen::prelude::*;

use lightning::util::ser::Readable;

use secp256k1_zkp::hashes::*;

use core::panic;
use std::{
    collections::HashMap,
    io::Cursor,
    sync::{Arc, Mutex},
};

use dlc_manager::{
    contract::{signed_contract::SignedContract, Contract},
    manager::Manager,
    ContractId, Oracle, Storage, SystemTimeProvider,
};

use std::fmt::Write as _;

use dlc_memory_storage_provider::DlcMemoryStorageProvider;
use log::info;

use dlc_blockchain_provider::DlcBlockchainProvider;

use js_interface_wallet::JSInterfaceWallet;

use oracle_client::P2PDOracleClient;
use serde::{Deserialize, Serialize};

mod oracle_client;
#[macro_use]
mod macros;

type DlcManager = Manager<
    Arc<JSInterfaceWallet>,
    Arc<DlcBlockchainProvider>,
    Box<DlcMemoryStorageProvider>,
    Arc<P2PDOracleClient>,
    Arc<SystemTimeProvider>,
    Arc<DlcBlockchainProvider>,
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
}

// #[wasm_bindgen]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JsDLCInterfaceOptions {
    oracle_url: String,
    network: String,
    electrs_url: String,
    address: String,
}

impl Default for JsDLCInterfaceOptions {
    // Default values for Manager Options
    fn default() -> Self {
        Self {
            oracle_url: "https://dev-oracle.dlc.link/oracle".to_string(),
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
        oracle_url: String,
    ) -> JsDLCInterface {
        console_error_panic_hook::set_once();

        clog!("Received JsDLCInterface parameters: privkey={}, address={}, network={}, electrs_url={}, oracle_url={}", privkey, address, network, electrs_url, oracle_url);

        let options = JsDLCInterfaceOptions {
            oracle_url,
            network,
            electrs_url,
            address,
        };

        let active_network: Network = options
            .network
            .parse::<Network>()
            .expect("Must use a valid bitcoin network");

        // Set up DLC store
        let store = DlcMemoryStorageProvider::new();

        // Set up wallet
        let wallet = Arc::new(JSInterfaceWallet::new(
            options.address.to_string(),
            options.electrs_url.to_string(),
            privkey,
            active_network,
        ));

        let esplora_blockchain = EsploraBlockchain::new(&options.electrs_url.clone(), 20);
        wallet
            .sync(&esplora_blockchain, options.address.clone())
            .await;

        let dlc_blockchain = Arc::new(DlcBlockchainProvider::new(Arc::clone(&wallet)));

        // Set up Oracle Client
        let p2p_client: P2PDOracleClient = P2PDOracleClient::new(&options.oracle_url.clone())
            .await
            .expect("To be able to connect to the oracle");

        let oracle = Arc::new(p2p_client);
        let oracles: HashMap<bitcoin::XOnlyPublicKey, _> =
            HashMap::from([(oracle.get_public_key(), oracle.clone())]);

        // Set up time provider
        let time_provider = SystemTimeProvider {};

        // Create the DLC Manager
        let manager = Arc::new(Mutex::new(
            Manager::new(
                Arc::clone(&wallet),
                Arc::clone(&dlc_blockchain),
                Box::new(store),
                oracles,
                Arc::new(time_provider),
                Arc::clone(&dlc_blockchain),
            )
            .unwrap(),
        ));
        clog!("Finished setting up manager");

        JsDLCInterface {
            options,
            manager,
            wallet,
        }
    }

    pub fn get_options(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.options).unwrap()
    }

    pub async fn get_wallet_balance(&self) -> u64 {
        // make sure to call sync for an updated value
        self.wallet.get_balance()
    }

    // public async function for fetching all the contracts on the manager
    pub async fn get_contracts(&self) -> JsValue {
        let contracts: Vec<JsContract> = self
            .manager
            .lock()
            .unwrap()
            .get_store()
            .get_contracts()
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
        clog!("Offer to accept: {:?}", dlc_offer_message);

        clog!("receive_offer - after on_dlc_message");
        let temporary_contract_id = dlc_offer_message.temporary_contract_id;

        match self.manager.lock().unwrap().on_dlc_message(
            &Message::Offer(dlc_offer_message.clone()),
            STATIC_COUNTERPARTY_NODE_ID.parse().unwrap(),
        ) {
            Ok(_) => (),
            Err(e) => {
                clog!("DLC manager - receive offer error: {}", e.to_string());
                return "".to_string();
            }
        }

        clog!("accepting contract with id {:?}", temporary_contract_id);

        let (_contract_id, _public_key, accept_msg) = self
            .manager
            .lock()
            .unwrap()
            .accept_contract_offer(&temporary_contract_id)
            .expect("Error accepting contract offer");

        clog!("receive_offer - after accept_contract_offer");
        serde_json::to_string(&accept_msg).unwrap()
    }

    pub async fn countersign_and_broadcast(&self, dlc_sign_message: String) -> String {
        clog!("sign_offer - before on_dlc_message");
        let dlc_sign_message: SignDlc = serde_json::from_str(&dlc_sign_message).unwrap();
        clog!("dlc_sign_message: {:?}", dlc_sign_message);
        match self.manager.lock().unwrap().on_dlc_message(
            &Message::Sign(dlc_sign_message.clone()),
            STATIC_COUNTERPARTY_NODE_ID.parse().unwrap(),
        ) {
            Ok(_) => (),
            Err(e) => {
                info!("DLC manager - sign offer error: {}", e.to_string());
                panic!()
            }
        }
        clog!("sign_offer - after on_dlc_message");
        let manager = self.manager.lock().unwrap();
        let store = manager.get_store();
        let contract: SignedContract = store
            .get_signed_contracts()
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
            .unwrap();

        match contract {
            Some(Contract::Offered(c)) => {
                self.manager
                    .lock()
                    .unwrap()
                    .get_store()
                    .update_contract(&Contract::Rejected(c))
                    .unwrap();
            }
            _ => (),
        };
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

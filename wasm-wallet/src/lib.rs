#![allow(unreachable_code)]
extern crate console_error_panic_hook;
extern crate log;

use bitcoin::{Address, Network};
use dlc_messages::{Message, OfferDlc, SignDlc};
use wasm_bindgen::prelude::{wasm_bindgen, JsValue};

use core::panic;
use std::{
    collections::HashMap,
    env,
    str::FromStr,
    sync::{Arc, Mutex},
};

use dlc_manager::{manager::Manager, Oracle, SystemTimeProvider, Wallet};

use log::info;
use mocks::memory_storage_provider::MemoryStorage;

use esplora_async_blockchain_provider::EsploraAsyncBlockchainProvider;

use simple_wallet::SimpleWallet;

use oracle_client::P2PDOracleClient;
use serde::{Deserialize, Serialize};

mod oracle_client;
mod utils;
#[macro_use]
mod macros;

type DlcManager = Manager<
    Arc<SimpleWallet<Arc<EsploraAsyncBlockchainProvider>, Arc<MemoryStorage>>>,
    Arc<EsploraAsyncBlockchainProvider>,
    Box<MemoryStorage>,
    Arc<P2PDOracleClient>,
    Arc<SystemTimeProvider>,
    Arc<EsploraAsyncBlockchainProvider>,
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

#[wasm_bindgen]
pub struct JsDLCInterface {
    options: JsDLCInterfaceOptions,
    manager: Arc<Mutex<DlcManager>>,
    wallet: Arc<SimpleWallet<Arc<EsploraAsyncBlockchainProvider>, Arc<MemoryStorage>>>,
    blockchain: Arc<EsploraAsyncBlockchainProvider>,
}

// #[wasm_bindgen]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JsDLCInterfaceOptions {
    oracle_url: String,
    network: String,
    electrs_host: String,
    address: String,
}

impl Default for JsDLCInterfaceOptions {
    fn default() -> Self {
        Self {
            oracle_url: "https://not-testnet.dlc.link/oracle".to_string(),
            // oracle_url: "http://localhost:8081".to_string(),
            network: "regtest".to_string(),
            electrs_host: "https://blockstream.info/testnet/api/".to_string(),
            address: "".to_string(),
        }
    }
}

#[wasm_bindgen]
impl JsDLCInterface {
    pub async fn new() -> JsDLCInterface {
        console_error_panic_hook::set_once();
        let mut options = JsDLCInterfaceOptions::default();
        let active_network: Network = options
            .network
            .parse::<Network>()
            .expect("Must use a valid bitcoin network");

        // ELECTRUM / ELECTRS ASYNC
        let electrs_host = env::var("ELECTRUM_API_URL")
            .unwrap_or("https://dev-oracle.dlc.link/electrs".to_string());
        let blockchain = Arc::new(EsploraAsyncBlockchainProvider::new(
            electrs_host.to_string(),
            active_network,
        ));

        // Set up DLC store
        let store = MemoryStorage::new();

        let wallet_store = Arc::new(MemoryStorage::new());

        // Set up wallet
        let wallet = Arc::new(SimpleWallet::new(
            blockchain.clone(),
            wallet_store.clone(),
            active_network,
        ));

        clog!("options: {:?}", options);
        let address = wallet.get_new_address().unwrap();
        clog!("address {}", address);
        options.address = address.to_string();

        // Set up Oracle Client
        let p2p_client: P2PDOracleClient = P2PDOracleClient::new(&options.oracle_url)
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
                Arc::clone(&blockchain),
                Box::new(store),
                oracles,
                Arc::new(time_provider),
                Arc::clone(&blockchain),
                None,
            )
            .unwrap(),
        ));

        clog!("Finished setting up manager");

        JsDLCInterface {
            options,
            manager,
            wallet,
            blockchain,
        }
    }

    pub fn oracle_url(&self) -> String {
        self.options.oracle_url.clone()
    }

    pub fn send_options_to_js(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.options).unwrap()
    }

    pub async fn get_wallet_balance(&self) -> u64 {
        self.blockchain
            .fetch_utxos_for_later(&Address::from_str(&self.options.address).unwrap())
            .await;
        self.wallet.refresh().unwrap();
        self.wallet.get_balance()
    }

    pub async fn receive_offer_and_accept(&self, offer_json: String) -> String {
        let dlc_offer_message: OfferDlc = serde_json::from_str(&offer_json).unwrap();
        clog!("received offer: {:?}", dlc_offer_message);
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

        clog!("receive_offer - after on_dlc_message");
        let temporary_contract_id = dlc_offer_message.temporary_contract_id;

        let (_contract_id, _public_key, accept_msg) = self
            .manager
            .lock()
            .unwrap()
            .accept_contract_offer(&temporary_contract_id)
            .expect("Error accepting contract offer");

        clog!("receive_offer - after accept_contract_offer");
        serde_json::to_string(&accept_msg).unwrap()
    }

    pub async fn countersign_and_broadcast(&self, dlc_sign_message: String) -> () {
        clog!("sign_offer - before on_dlc_message");
        let dlc_sign_message: SignDlc = serde_json::from_str(&dlc_sign_message).unwrap();
        clog!("dlc_sign_message: {:?}", dlc_sign_message);
        match self.manager.lock().unwrap().on_dlc_message(
            &Message::Sign(dlc_sign_message),
            STATIC_COUNTERPARTY_NODE_ID.parse().unwrap(),
        ) {
            Ok(_) => (),
            Err(e) => {
                info!("DLC manager - sign offer error: {}", e.to_string());
                panic!()
            }
        }
        clog!("sign_offer - after on_dlc_message");
    }
}

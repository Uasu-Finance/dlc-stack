#![allow(unreachable_code)]
extern crate console_error_panic_hook;
extern crate log;

use bitcoin::{Address, Network};
use dlc_messages::{Message, OfferDlc, SignDlc};
use serde_json::json;
use wasm_bindgen::prelude::{wasm_bindgen, JsValue};

use lightning::util::ser::Readable;

use secp256k1_zkp::{
    hashes::*, All, KeyPair, Secp256k1, SecretKey, XOnlyPublicKey as SchnorrPublicKey,
};

use core::panic;
use std::{
    collections::HashMap,
    env,
    io::Cursor,
    str::FromStr,
    sync::{Arc, Mutex},
};

use dlc_manager::{
    contract::{offered_contract::OfferedContract, Contract},
    manager::Manager,
    ContractId, Oracle, Storage, SystemTimeProvider, Wallet,
};

use std::fmt::Write as _;

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
    // Default values for Manager Options
    fn default() -> Self {
        Self {
            oracle_url: "https://testnet.dlc.link/oracle".to_string(),
            network: "regtest".to_string(),
            electrs_host: "https://dev-oracle.dlc.link/electrs".to_string(),
            address: "".to_string(),
        }
    }
}

#[wasm_bindgen]
impl JsDLCInterface {
    pub async fn new() -> JsDLCInterface {
        //privkey: String) -> JsDLCInterface {
        console_error_panic_hook::set_once();

        let mut options = JsDLCInterfaceOptions::default();
        let active_network: Network = options
            .network
            .parse::<Network>()
            .expect("Must use a valid bitcoin network");

        // ELECTRUM / ELECTRS ASYNC
        options.electrs_host = env::var("ELECTRUM_API_URL")
            .unwrap_or("https://dev-oracle.dlc.link/electrs".to_string());

        let blockchain: Arc<EsploraAsyncBlockchainProvider> = Arc::new(
            EsploraAsyncBlockchainProvider::new(options.electrs_host.to_string(), active_network),
        );

        // Set up DLC store
        let store = MemoryStorage::new();
        let wallet_store = Arc::new(MemoryStorage::new());

        // Pass the private key to the simplewallet constructor, upsert the keypair. Don't create additional keypairs

        // Set up wallet
        let wallet = Arc::new(SimpleWallet::new(
            blockchain.clone(),
            wallet_store.clone(),
            active_network,
        ));

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

    pub fn get_options(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.options).unwrap()
    }

    pub async fn get_wallet_balance(&self) -> u64 {
        self.blockchain
            .fetch_utxos_for_later(&Address::from_str(&self.options.address).unwrap())
            .await;
        self.wallet.refresh().unwrap();
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

    // acceptContract(contractId: string, btcAddress: string, btcPublicKey: string, btcPrivateKey: string, btcNetwork: NetworkType): Promise<AnyContract>

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

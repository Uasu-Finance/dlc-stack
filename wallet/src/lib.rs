#![allow(unreachable_code)]
extern crate log;

#[macro_use]
extern crate rouille;

use std::{
    cmp,
    collections::HashMap,
    env, panic,
    str::FromStr,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
    vec,
};

use bitcoin::Address;
use dlc_manager::{
    contract::contract_input::{ContractInput, ContractInputInfo, OracleInput},
    manager::Manager,
    Oracle, SystemTimeProvider, Wallet,
};
use dlc_messages::{AcceptDlc, Message, OfferDlc, SignDlc};
use dlc_sled_storage_provider::SledStorageProvider;
// use electrs_blockchain_provider::ElectrsBlockchainProvider;
use lightning::chain::chaininterface::{ConfirmationTarget, FeeEstimator};
use log::{debug, info, warn};
use mock_blockchain_provider::MockBlockchainProvider;
use simple_wallet::SimpleWallet;

// use crate::storage::storage_provider::StorageProvider;
use oracle_client::P2PDOracleClient;
use rouille::Response;
use serde::{Deserialize, Serialize};
use utils::get_numerical_contract_info;

mod oracle_client;
// mod storage;
mod utils;
#[macro_use]
mod macros;

type DlcManager<'a> = Manager<
    Arc<SimpleWallet<Arc<MockBlockchainProvider>, Arc<SledStorageProvider>>>,
    Arc<MockBlockchainProvider>,
    Box<SledStorageProvider>,
    Arc<P2PDOracleClient>,
    Arc<SystemTimeProvider>,
    Arc<MockBlockchainProvider>,
>;

const NUM_CONFIRMATIONS: u32 = 2;

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

fn main() {
    env_logger::init();
    let oracle_url: String = env::var("ORACLE_URL").unwrap_or("http://localhost:8080".to_string());

    let wallet_backend_port: String = env::var("WALLET_BACKEND_PORT").unwrap_or("8085".to_string());

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

    // ELECTRUM / ELECTRS
    let electrs_host =
        env::var("ELECTRUM_API_URL").unwrap_or("https://blockstream.info/testnet/api/".to_string());
    let blockchain = Arc::new(MockBlockchainProvider::new(
        electrs_host.to_string(),
        active_network,
    ));

    // Set up DLC store
    let store =
        Box::new(SledStorageProvider::new("dlc_db")).expect("Create a SledStorageProvider object");

    // Set up wallet store
    let root_sled_path: String = env::var("SLED_WALLET_PATH").unwrap_or("wallet_db".to_string());
    let sled_path = format!("{root_sled_path}_{}", active_network);
    let wallet_store = Arc::new(SledStorageProvider::new(sled_path.as_str()).unwrap());

    // Set up wallet
    let wallet = Arc::new(SimpleWallet::new(
        blockchain.clone(),
        wallet_store.clone(),
        active_network,
    ));

    let wallet2 = wallet.clone();

    // Set up Oracle Client
    let p2p_client: P2PDOracleClient = retry!(
        P2PDOracleClient::new(&oracle_url),
        10,
        "oracle client creation"
    );
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
        )
        .unwrap(),
    ));

    // Start periodic_check thread
    let bitcoin_check_interval_seconds: u64 = env::var("BITCOIN_CHECK_INTERVAL_SECONDS")
        .unwrap_or("10".to_string())
        .parse::<u64>()
        .unwrap_or(10);

    info!("periodic_check loop thread starting");
    debug!("Wallet address: {:?}", wallet.get_new_address());
    thread::spawn(move || loop {
        debug!("Wallet balance: {}", wallet.get_balance());
        wallet
            .refresh()
            .unwrap_or_else(|e| warn!("Error refreshing wallet {e}"));
        thread::sleep(Duration::from_millis(
            cmp::max(10, bitcoin_check_interval_seconds) * 1000,
        ));
    });

    rouille::start_server(format!("0.0.0.0:{}", wallet_backend_port), move |request| {
        router!(request,
                // (GET) (/cleanup) => {
                //     let contract_cleanup_enabled: bool = env::var("CONTRACT_CLEANUP_ENABLED")
                //         .unwrap_or("false".to_string())
                //         .parse().unwrap_or(false);
                //     if contract_cleanup_enabled {
                //         info!("Call cleanup contract offers.");
                //         delete_all_offers(manager.clone(), Response::json(&("OK".to_string())).with_status_code(200))
                //     } else {
                //         info!("Call cleanup contract offers feature disabled.");
                //         Response::json(&("Disabled".to_string())).with_status_code(400)
                //     }
                // },
                (GET) (/unlockutxos) => {
                    unlock_utxos(wallet2.clone(), Response::json(&("OK".to_string())).with_status_code(200))
                },
                (GET) (/empty_to_address/{address: String}) => {
                    empty_to_address(address, wallet2.clone(), Response::json(&("OK".to_string())).with_status_code(200))
                },
                (POST) (/offer) => {
                    info!("Call POST (create) offer {:?}", request);
                    #[derive(Deserialize)]
                    #[serde(rename_all = "camelCase")]
                    struct OfferRequest {
                        uuid: String,
                        accept_collateral: u64,
                        offer_collateral: u64,
                        total_outcomes: u64
                    }
                    let req: OfferRequest = try_or_400!(rouille::input::json_input(request));
                    add_access_control_headers(create_new_offer(manager.clone(), oracle.clone(), blockchain.clone(), active_network, req.uuid, req.accept_collateral, req.offer_collateral, req.total_outcomes))
                },
                (OPTIONS) (/offer) => {
                    add_access_control_headers(Response::empty_204())
                },
                (OPTIONS) (/offer/accept) => {
                    add_access_control_headers(Response::empty_204())
                },
                (PUT) (/offer/receive) => {
                    info!("Call PUT (receive) offer {:?}", request);
                    #[derive(Deserialize)]
                    #[serde(rename_all = "camelCase")]
                    struct ReceiveOfferRequest {
                        receive_offer_message: String,
                    }
                    let json: ReceiveOfferRequest = try_or_400!(rouille::input::json_input(request));
                    let receive_offer_dlc: OfferDlc = match serde_json::from_str(&json.receive_offer_message)
                    {
                        Ok(dlc) => dlc,
                        Err(e) => return add_access_control_headers(Response::json(&ErrorsResponse{status: 400, errors: vec![ErrorResponse{message: e.to_string(), code: None}]}).with_status_code(400)),
                    };
                    receive_offer(receive_offer_dlc, manager.clone())
                },
                (PUT) (/offer/sign) => {
                    info!("Call PUT (sign) offer {:?}", request);
                    #[derive(Deserialize)]
                    #[serde(rename_all = "camelCase")]
                    struct SignOfferRequest {
                        sign_message: String,
                    }
                    let json: SignOfferRequest = try_or_400!(rouille::input::json_input(request));
                    let sign_dlc: SignDlc = match serde_json::from_str(&json.sign_message)
                    {
                        Ok(dlc) => dlc,
                        Err(e) => return add_access_control_headers(Response::json(&ErrorsResponse{status: 400, errors: vec![ErrorResponse{message: e.to_string(), code: None}]}).with_status_code(400)),
                    };
                    sign_offer(sign_dlc, manager.clone())
                },
                _ => rouille::Response::empty_404()
        )
    });
}

fn create_new_offer(
    manager: Arc<Mutex<DlcManager>>,
    oracle: Arc<P2PDOracleClient>,
    blockchain: Arc<MockBlockchainProvider>,
    active_network: bitcoin::Network,
    event_id: String,
    accept_collateral: u64,
    offer_collateral: u64,
    total_outcomes: u64,
) -> Response {
    let (_event_descriptor, descriptor) =
        get_numerical_contract_info(accept_collateral, offer_collateral, total_outcomes);
    info!(
        "Creating new offer with event id: {}, accept collateral: {}, offer_collateral: {}",
        event_id.clone(),
        accept_collateral,
        offer_collateral
    );

    let contract_info = ContractInputInfo {
        oracles: OracleInput {
            public_keys: vec![oracle.get_public_key()],
            event_id: event_id.clone(),
            threshold: 1,
        },
        contract_descriptor: descriptor,
    };

    // Some regtest networks have an unreliable fee estimation service
    let fee_rate = match active_network {
        bitcoin::Network::Regtest => 1,
        _ => blockchain.get_est_sat_per_1000_weight(ConfirmationTarget::Normal) as u64,
    };

    let contract_input = ContractInput {
        offer_collateral: offer_collateral,
        accept_collateral: accept_collateral,
        fee_rate,
        contract_infos: vec![contract_info],
    };

    match &manager.lock().unwrap().send_offer(
        &contract_input,
        STATIC_COUNTERPARTY_NODE_ID.parse().unwrap(),
    ) {
        Ok(dlc) => Response::json(dlc),
        Err(e) => {
            info!("DLC manager - send offer error: {}", e.to_string());
            Response::json(&ErrorsResponse {
                status: 400,
                errors: vec![ErrorResponse {
                    message: e.to_string(),
                    code: None,
                }],
            })
            .with_status_code(400)
        }
    }
}

fn receive_offer(dlc_offer_message: OfferDlc, manager: Arc<Mutex<DlcManager>>) -> Response {
    match manager.lock().unwrap().on_dlc_message(
        &Message::Offer(dlc_offer_message.clone()),
        STATIC_COUNTERPARTY_NODE_ID.parse().unwrap(),
    ) {
        Ok(_) => (),
        Err(e) => {
            info!("DLC manager - receive offer error: {}", e.to_string());
            return add_access_control_headers(
                Response::json(&ErrorsResponse {
                    status: 400,
                    errors: vec![ErrorResponse {
                        message: e.to_string(),
                        code: None,
                    }],
                })
                .with_status_code(400),
            );
        }
    }

    let temporary_contract_id = dlc_offer_message.temporary_contract_id;

    let (_, _, accept_msg) = manager
        .lock()
        .unwrap()
        .accept_contract_offer(&temporary_contract_id)
        .expect("Error accepting contract offer");

    add_access_control_headers(Response::json(&accept_msg))
}

fn sign_offer(dlc_sign_message: SignDlc, manager: Arc<Mutex<DlcManager>>) -> Response {
    match manager.lock().unwrap().on_dlc_message(
        &Message::Sign(dlc_sign_message),
        STATIC_COUNTERPARTY_NODE_ID.parse().unwrap(),
    ) {
        Ok(_) => add_access_control_headers(Response::json(&"OK".to_string())),
        Err(e) => {
            info!("DLC manager - sign offer error: {}", e.to_string());
            return add_access_control_headers(
                Response::json(&ErrorsResponse {
                    status: 400,
                    errors: vec![ErrorResponse {
                        message: e.to_string(),
                        code: None,
                    }],
                })
                .with_status_code(400),
            );
        }
    }
}

// Can remove this when we implement BDK, assuming BDK also doesn't do reserving (locking) of utxos
fn unlock_utxos(
    wallet: Arc<SimpleWallet<Arc<MockBlockchainProvider>, Arc<SledStorageProvider>>>,
    response: Response,
) -> Response {
    info!("Unlocking UTXOs");
    wallet.unreserve_all_utxos();
    return response;
}

fn empty_to_address(
    address: String,
    wallet: Arc<SimpleWallet<Arc<MockBlockchainProvider>, Arc<SledStorageProvider>>>,
    response: Response,
) -> Response {
    info!("Unlocking UTXOs");
    match wallet.empty_to_address(&Address::from_str(&address).unwrap()) {
        Ok(_) => info!("Emptied bitcoin to {address}"),
        Err(_) => warn!("Failed emptying bitcoin to {address}"),
    }
    return response;
}

fn add_access_control_headers(response: Response) -> Response {
    return response
        .with_additional_header("Access-Control-Allow-Origin", "*")
        .with_additional_header("Access-Control-Allow-Methods", "*")
        .with_additional_header("Access-Control-Allow-Headers", "*");
}

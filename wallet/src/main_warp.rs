#![feature(async_fn_in_trait)]
#![allow(unreachable_code)]
extern crate log;

use bdk::Wallet as BdkWallet;
use bdk::{blockchain::esplora::EsploraBlockchain, SyncOptions};
use bdk::{descriptor::IntoWalletDescriptor, wallet::AddressIndex};
use core::fmt;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::{
    clone, cmp,
    collections::HashMap,
    convert::Infallible,
    env, panic,
    str::FromStr,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
    vec,
};
use tokio::sync::RwLock;
use tokio::{runtime, task};
use warp::{sse::Event, Filter};

use bitcoin::{hashes::Hash, KeyPair, Network, XOnlyPublicKey};
use dlc_bdk_wallet::DlcBdkWallet;
// use dlc_link_manager::Manager;
use dlc_link_manager::{AsyncStorage, Manager};
use dlc_manager::{
    contract::{
        contract_input::{ContractInput, ContractInputInfo, OracleInput},
        Contract,
    },
    // manager::Manager,
    Blockchain,
    Oracle,
    Storage,
    SystemTimeProvider,
};
use dlc_messages::{AcceptDlc, Message};
use dlc_sled_storage_provider::SledStorageProvider;
// use electrs_blockchain_provider::ElectrsBlockchainProvider;
use esplora_async_blockchain_provider::EsploraAsyncBlockchainProvider;
use log::{debug, error, info, warn};

// use crate::storage::storage_provider::StorageProvider;
use oracle_client::P2PDOracleClient;
use serde_json::{json, Value};
use std::fmt::Write as _;
use storage::async_storage_api::AsyncStorageApiProvider;
use utils::get_numerical_contract_info;

mod oracle_client;
mod storage;
mod utils;
#[macro_use]
mod macros;

// remove lifetime?
type DlcManager<'a> = Manager<
    Arc<DlcBdkWallet>,
    Arc<EsploraAsyncBlockchainProvider>,
    Box<AsyncStorageApiProvider>,
    Arc<P2PDOracleClient>,
    Arc<SystemTimeProvider>,
    // Arc<EsploraAsyncBlockchainProvider>,
>;

// struct TestThing {
//     test: String,
//     hash: Arc<HashMap<String, Arc<Mutex<EsploraAsyncBlockchainProvider>>>>, // whahhaha
// }
// impl fmt::Debug for TestThing {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         f.debug_struct("TestThing")
//             .field("test", &self.test)
//             .finish()
//     }
// }
// impl clone::Clone for TestThing {
//     fn clone(&self) -> Self {
//         TestThing {
//             test: self.test.clone(),
//             hash: self.hash.clone(),
//         }
//     }
// }
// type WrappedThing = TestThing;

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

pub fn to_oracle_error<T>(e: T) -> dlc_manager::error::Error
where
    T: std::fmt::Display,
{
    dlc_manager::error::Error::OracleError(e.to_string())
}

async fn get_attestors() -> Result<Vec<String>, dlc_manager::error::Error> {
    let blockchain_interface_url = env::var("BLOCKCHAIN_INTERFACE_URL")
        .expect("BLOCKCHAIN_INTERFACE_URL environment variable not set, couldn't get attestors");

    let get_all_attestors_endpoint_url = format!("{}/get-all-attestors", blockchain_interface_url);

    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .build()
        .map_err(to_oracle_error)?;

    let res = client
        .get(get_all_attestors_endpoint_url.as_str())
        .send()
        .await
        .map_err(to_oracle_error)?;

    let attestors = res.json::<Vec<String>>().await.map_err(to_oracle_error)?;

    match attestors.len() {
        0 => Err(dlc_manager::error::Error::OracleError(
            "No attestors found".to_string(),
        )),
        _ => Ok(attestors),
    }
}

async fn generate_p2pd_clients(
    attestor_urls: Vec<String>,
) -> HashMap<XOnlyPublicKey, Arc<P2PDOracleClient>> {
    let mut attestor_clients = HashMap::new();

    for url in attestor_urls.iter() {
        let p2p_client: P2PDOracleClient = P2PDOracleClient::new(url).await.unwrap();
        let attestor = Arc::new(p2p_client);
        attestor_clients.insert(attestor.get_public_key(), attestor.clone());
    }
    return attestor_clients;
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let local = task::LocalSet::new();
    local
        .run_until(async move {
            task::spawn_local(async move {
                env_logger::init();

                // initialize tracing
                // tracing_subscriber::fmt::init();

                let wallet_backend_port: String = env::var("WALLET_BACKEND_PORT").unwrap_or("8085".to_string());

                let wallet_descriptor_string = env::var("WALLET_DESCRIPTOR")
                    .expect("WALLET_DESCRIPTOR environment variable not set, please run `just generate-descriptor`, securely backup the output, and set this env_var accordingly");

                let wallet_pkey = env::var("WALLET_PKEY")
                    .expect("WALLET_PKEY environment variable not set, please run `just generate-descriptor`, securely backup the output, and set this env_var accordingly");

                let secp = bitcoin::secp256k1::Secp256k1::new();
                let (wallet_desc, keymap) = wallet_descriptor_string
                    .into_wallet_descriptor(&secp, Network::Testnet)
                    .unwrap();

                println!("wallet_desc: {:?}", wallet_desc);
                println!("\n\nkeymap: {:?}", keymap);
                let first_key = keymap.keys().next().unwrap();
                // this is creating a 66 hex-character pubkey, but in attestor we are currently creating an xpubkey with only 64 characters
                let pubkey = first_key
                    .clone()
                    .at_derivation_index(0)
                    .derive_public_key(&secp)
                    .unwrap()
                    .inner
                    .to_string();

                let keypair = KeyPair::from_seckey_str(&secp, &wallet_pkey).unwrap();

                let seckey = keypair.secret_key();

                let sled = sled::open("my-database")
                    .unwrap()
                    .open_tree("default_tree")
                    .unwrap();

                let attestor_urls: Vec<String> = get_attestors().await.unwrap();

                let blockchain_interface_url = env::var("BLOCKCHAIN_INTERFACE_URL")
                    .expect("BLOCKCHAIN_INTERFACE_URL environment variable not set, couldn't get attestors");

                let funded_endpoint_url = format!("{}/set-status-funded", blockchain_interface_url);

                let mut funded_uuids: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));

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
                let blockchain = Arc::new(EsploraAsyncBlockchainProvider::new(
                    electrs_host.to_string(),
                    active_network,
                ));

                let bdk_wallet = Arc::new(Mutex::new(
                    BdkWallet::new(wallet_desc, None, Network::Testnet, sled).unwrap(),
                ));

                let static_address = bdk_wallet
                    .lock()
                    .unwrap()
                    .get_address(AddressIndex::Peek(0))
                    .unwrap();
                println!("Address: {}", static_address);

                // Set up wallet store
                let root_sled_path: String = env::var("SLED_WALLET_PATH").unwrap_or("wallet_db".to_string());
                let sled_path = format!("{root_sled_path}_{}", active_network);
                // let _wallet_store = Arc::new(SledStorageProvider::new(sled_path.as_str()).unwrap());
                let wallet: Arc<DlcBdkWallet> = Arc::new(DlcBdkWallet::new(
                    bdk_wallet,
                    static_address.clone(),
                    seckey.clone(),
                    active_network,
                ));

                // Set up Oracle Client
                let protocol_wallet_attestors = generate_p2pd_clients(attestor_urls.clone()).await;

                // Set up time provider
                let time_provider = SystemTimeProvider {};

                // retry!(
                // blockchain.get_blockchain_height(),
                //     10,
                //     "get blockchain height"
                // );

                // Set up DLC store
                let dlc_store = Box::new(AsyncStorageApiProvider::new(
                    pubkey.to_string(),
                    "https://devnet.dlc.link/storage-api".to_string(),
                ));

                // Create the DLC Manager
                let manager = Arc::new(Mutex::new(
                    Manager::new(
                        Arc::clone(&wallet),
                        Arc::clone(&blockchain),
                        dlc_store.clone(),
                        protocol_wallet_attestors.clone(),
                        Arc::new(time_provider),
                        // Arc::clone(&blockchain),
                    )
                    .unwrap(),
                ));

                // Start periodic_check thread
                let bitcoin_check_interval_seconds: u64 = env::var("BITCOIN_CHECK_INTERVAL_SECONDS")
                    .unwrap_or("10".to_string())
                    .parse::<u64>()
                    .unwrap_or(10);

                let manager2 = manager.clone();
                let wallet2 = wallet.clone();
                info!("Please query '/info' endpoint to get wallet info");
                info!("periodic_check loop thread starting");
                let bdk_blockchain = EsploraBlockchain::new(&electrs_host, 20);
                // thread::spawn(move || loop {
                //     periodic_check(
                //         manager2.clone(),
                //         funded_endpoint_url.clone(),
                //         &mut funded_uuids,
                //     );
                //     wallet
                //         .bdk_wallet
                //         .lock()
                //         .unwrap()
                //         .sync(&bdk_blockchain, SyncOptions::default())
                //         .await
                //         .unwrap_or_else(|e| warn!("Error refreshing wallet {e}"));
                //     thread::sleep(Duration::from_millis(
                //         cmp::max(10, bitcoin_check_interval_seconds) * 1000,
                //     ));
                // });

                let manager_state = warp::any().map(move || manager.clone());
                let wallet_state = warp::any().map(move || wallet.clone());
                let store_state = warp::any().map(move || dlc_store.clone());
                let funded_endpoint_url_state = warp::any().map(move || funded_endpoint_url.clone());
                let funded_uuids_state = warp::any().map(move || funded_uuids.clone());

                let cleanup = warp::path("cleanup")
                    .and(warp::get())
                    .and(store_state.clone())
                    .then(|store| async move {
                        delete_all_offers(store).await;
                        println!("man = asdf",);
                        warp::reply()
                    });

                let info = warp::path("info")
                    .and(warp::get())
                    .and(store_state.clone())
                    .and(wallet_state)
                    .and_then(|store, wal| async move { get_wallet_info(store, wal).await });

                let periodic = warp::path("periodic_check")
                    .and(warp::get())
                    .and(manager_state.clone())
                    .and(store_state.clone())
                    .and_then(|manager, store| async move { periodic_check(manager, store).await })
                    .and(funded_endpoint_url_state)
                    .and(funded_uuids_state)
                    .and_then(
                        |newly_confirmed_uuids: Vec<String>,
                        funded_endpoint: String,
                        funded_uuids: Arc<Mutex<Vec<String>>>| async move {
                            // let thing = newly_confirmed_uuids.clone();
                            funded_uuids
                                .lock()
                                .unwrap()
                                .extend_from_slice(&newly_confirmed_uuids);
                            update_funded_uuids(funded_endpoint.clone(), newly_confirmed_uuids.clone()).await
                        },
                    );

                // GET / -> index html
                let index = warp::path::end().map(|| {
                    warp::http::Response::builder()
                        .header("content-type", "text/html; charset=utf-8")
                        .body("<html></html>")
                });

                let routes = index.or(cleanup).or(info).or(periodic);

                warp::serve(routes).run(([0, 0, 0, 0], 8085)).await;
            })
            .await
            .unwrap();
        })
        .await;
}

async fn periodic_check(
    manager: Arc<Mutex<DlcManager<'_>>>,
    store: Box<AsyncStorageApiProvider>,
) -> Result<Vec<String>, Infallible> {
    let mut man = manager.lock().unwrap();

    let updated_contract_ids = match man.periodic_check().await {
        Ok(updated_contract_ids) => updated_contract_ids,
        Err(e) => {
            info!("Error in periodic_check, will retry: {}", e.to_string());
            vec![]
        }
    };
    let updated_contract_ids = vec![];
    let mut newly_confirmed_uuids = vec![];

    for id in updated_contract_ids {
        let contract = match store.get_contract(&id).await {
            Ok(Some(contract)) => contract,
            Ok(None) => {
                error!("Error retrieving contract: {:?}", id);
                continue;
            }
            Err(e) => {
                error!("Error retrieving contract: {}", e.to_string());
                continue;
            }
        };

        let found_uuid = match contract {
            Contract::Confirmed(c) => c
                .accepted_contract
                .offered_contract
                .contract_info
                .iter()
                .next()
                .map_or(None, |ci| {
                    ci.oracle_announcements
                        .iter()
                        .next()
                        .map_or(None, |oa| Some(oa.oracle_event.event_id.clone()))
                }),
            _ => None,
        };
        if found_uuid.is_none() {
            error!(
                "Error retrieving contract in periodic_check: {:?}, skipping",
                id
            );
        }
        newly_confirmed_uuids.push(found_uuid.unwrap());
    }
    Ok(newly_confirmed_uuids)
}

async fn update_funded_uuids(
    funded_url: String,
    newly_confirmed_uuids: Vec<String>,
) -> Result<impl warp::Reply, Infallible> {
    for uuid in newly_confirmed_uuids.clone() {
        debug!("Contract is funded, setting funded to true: {}", uuid);
        let mut post_body = HashMap::new();
        post_body.insert("uuid", &uuid);

        let client = reqwest::Client::builder().use_rustls_tls().build();
        if client.is_ok() {
            let res = client
                .unwrap()
                .post(&funded_url)
                .json(&post_body)
                .send()
                .await;

            match res {
                Ok(res) => match res.error_for_status() {
                    Ok(_res) => {
                        info!(
                            "Success setting funded to true: {}, {}",
                            uuid,
                            _res.status()
                        );
                    }
                    Err(e) => {
                        info!("Error setting funded to true: {}: {}", uuid, e.to_string());
                    }
                },
                Err(e) => {
                    info!("Error setting funded to true: {}: {}", uuid, e.to_string());
                }
            }
        }
    }
    Ok(warp::reply::json(&newly_confirmed_uuids))
}

async fn get_wallet_info(
    store: Box<AsyncStorageApiProvider>,
    wallet: Arc<DlcBdkWallet>,
    // static_address: String,
) -> Result<impl warp::Reply, Infallible> {
    let mut info_response = json!({});
    let mut contracts_json = json!({});

    fn hex_str(value: &[u8]) -> String {
        let mut res = String::with_capacity(64);
        for v in value {
            write!(res, "{:02x}", v).unwrap();
        }
        res
    }

    let mut collected_contracts: Vec<Vec<String>> = vec![
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
    ];

    let contracts = store
        .get_contracts()
        .await
        .expect("Error retrieving contract list.");

    for contract in contracts {
        let id = hex_str(&contract.get_id());
        match contract {
            Contract::Offered(_) => {
                collected_contracts[0].push(id);
            }
            Contract::Accepted(_) => {
                collected_contracts[1].push(id);
            }
            Contract::Confirmed(_) => {
                collected_contracts[2].push(id);
            }
            Contract::Signed(_) => {
                collected_contracts[3].push(id);
            }
            Contract::Closed(_) => {
                collected_contracts[4].push(id);
            }
            Contract::Refunded(_) => {
                collected_contracts[5].push(id);
            }
            Contract::FailedAccept(_) | Contract::FailedSign(_) => {
                collected_contracts[6].push(id);
            }
            Contract::Rejected(_) => collected_contracts[7].push(id),
            Contract::PreClosed(_) => collected_contracts[8].push(id),
        }
    }

    contracts_json["Offered"] = collected_contracts[0].clone().into();
    contracts_json["Accepted"] = collected_contracts[1].clone().into();
    contracts_json["Confirmed"] = collected_contracts[2].clone().into();
    contracts_json["Signed"] = collected_contracts[3].clone().into();
    contracts_json["Closed"] = collected_contracts[4].clone().into();
    contracts_json["Refunded"] = collected_contracts[5].clone().into();
    contracts_json["Failed"] = collected_contracts[6].clone().into();
    contracts_json["Rejected"] = collected_contracts[7].clone().into();
    contracts_json["PreClosed"] = collected_contracts[8].clone().into();

    info_response["wallet"] = json!({
        "balance": wallet.bdk_wallet.lock().unwrap().get_balance().unwrap().confirmed,
        "address": wallet.address
    });
    info_response["contracts"] = contracts_json;

    // Response::json(&info_response)
    Ok(info_response.to_string())
}
// fn create_new_offer(
//     manager: Arc<Mutex<DlcManager>>,
//     attestors: Vec<Arc<P2PDOracleClient>>,
//     active_network: bitcoin::Network,
//     event_id: String,
//     accept_collateral: u64,
//     offer_collateral: u64,
//     total_outcomes: u64,
// ) -> Response {
//     let (_event_descriptor, descriptor) = get_numerical_contract_info(
//         accept_collateral,
//         offer_collateral,
//         total_outcomes,
//         attestors.len(),
//     );
//     info!(
//         "Creating new offer with event id: {}, accept collateral: {}, offer_collateral: {}",
//         event_id.clone(),
//         accept_collateral,
//         offer_collateral
//     );

//     let contract_info = ContractInputInfo {
//         oracles: OracleInput {
//             public_keys: attestors.iter().map(|o| o.get_public_key()).collect(),
//             event_id: event_id.clone(),
//             threshold: attestors.len() as u16,
//         },
//         contract_descriptor: descriptor,
//     };

//     for attestor in attestors {
//         // check if the oracle has an event with the id of event_id
//         match attestor.get_announcement(&event_id) {
//             Ok(_announcement) => (),
//             Err(e) => {
//                 info!("Error getting announcement: {}", event_id);
//                 return Response::json(
//                     &(ErrorsResponse {
//                         status: 400,
//                         errors: vec![ErrorResponse {
//                             message: format!(
//                                 "Error: unable to get announcement. Does it exist? -- {}",
//                                 e.to_string()
//                             ),
//                             code: None,
//                         }],
//                     }),
//                 )
//                 .with_status_code(400);
//             }
//         }
//     }

//     // Some regtest networks have an unreliable fee estimation service
//     let fee_rate = match active_network {
//         bitcoin::Network::Regtest => 1,
//         _ => 400,
//     };

//     println!("contract_info: {:?}", contract_info);

//     let contract_input = ContractInput {
//         offer_collateral: offer_collateral,
//         accept_collateral: accept_collateral,
//         fee_rate,
//         contract_infos: vec![contract_info],
//     };

//     match &manager.lock().unwrap().send_offer(
//         &contract_input,
//         STATIC_COUNTERPARTY_NODE_ID.parse().unwrap(),
//     ) {
//         Ok(dlc) => Response::json(&(dlc)),
//         Err(e) => {
//             info!("DLC manager - send offer error: {}", e.to_string());
//             Response::json(
//                 &(ErrorsResponse {
//                     status: 400,
//                     errors: vec![ErrorResponse {
//                         message: e.to_string(),
//                         code: None,
//                     }],
//                 }),
//             )
//             .with_status_code(400)
//         }
//     }
// }

// fn accept_offer(accept_dlc: AcceptDlc, manager: Arc<Mutex<DlcManager>>) -> Response {
//     println!("accept_dlc: {:?}", accept_dlc);
//     if let Some(Message::Sign(sign)) = match manager.lock().unwrap().on_dlc_message(
//         &Message::Accept(accept_dlc),
//         STATIC_COUNTERPARTY_NODE_ID.parse().unwrap(),
//     ) {
//         Ok(dlc) => dlc,
//         Err(e) => {
//             info!("DLC manager - accept offer error: {}", e.to_string());
//             return add_access_control_headers(
//                 warp::reply::json(
//                     &(ErrorsResponse {
//                         status: 400,
//                         errors: vec![ErrorResponse {
//                             message: e.to_string(),
//                             code: None,
//                         }],
//                     }),
//                 )
//                 .with_status_code(400),
//             );
//         }
//     } {
//         add_access_control_headers(Response::json(&sign))
//     } else {
//         return Response::json(
//             &(ErrorsResponse {
//                 status: 400,
//                 errors: vec![ErrorResponse {
//                     message: format!("Error: invalid Sign message for accept_offer function"),
//                     code: None,
//                 }],
//             }),
//         )
//         .with_status_code(400);
//     }
// }

async fn delete_all_offers(store: Box<AsyncStorageApiProvider>) -> () {
    info!("Deleting all contracts from dlc-store");
    // let man = manager.lock().unwrap();
    store.delete_contracts().await;
    // (
    //     StatusCode::OK,
    //     Json(SimpleResponse {
    //         message: "OK".to_string(),
    //     }),
    // )
}

#[derive(Serialize, Deserialize)]
struct SimpleResponse {
    message: String,
}

// fn unlock_utxos(wallet: Arc<DlcBdkWallet>, response: Response) -> Response {
//     info!("Unlocking UTXOs");
//     wallet.unreserve_all_utxos();
//     return response;
// }

// fn empty_to_address(address: String, wallet: Arc<DlcBdkWallet>, response: Response) -> Response {
//     info!("Unlocking UTXOs");
//     match wallet.empty_to_address(&Address::from_str(&address).unwrap()) {
//         Ok(_) => info!("Emptied bitcoin to {address}"),
//         Err(_) => warn!("Failed emptying bitcoin to {address}"),
//     }
//     return response;
// }

// fn add_access_control_headers(response: Response) -> Response {
//     return response
//         .with_additional_header("Access-Control-Allow-Origin", "*")
//         .with_additional_header("Access-Control-Allow-Methods", "*")
//         .with_additional_header("Access-Control-Allow-Headers", "*");
// }

// let app = Router::new()
//     // `GET /` goes to `root`
//     .route("/", get(delete_all_offers))
//     .with_state(manager.clone());
// `POST /users` goes to `create_user`
// .route("/users", post(create_user));

// run our app with hyper, listening globally on port 3000
// let listener = tokio::net::TcpListener::bind("0.0.0.0:8085").await.unwrap();
// axum::serve(listener, app).await.unwrap();

//     rouille::start_server(format!("0.0.0.0:{}", wallet_backend_port), move |request| {
//         router!(request,
//                 (GET) (/cleanup) => {
//                     let contract_cleanup_enabled: bool = env::var("CONTRACT_CLEANUP_ENABLED")
//                         .unwrap_or("false".to_string())
//                         .parse().unwrap_or(false);
//                     if contract_cleanup_enabled {
//                         info!("Call cleanup contract offers.");
//                         delete_all_offers(manager.clone(), Response::json(&("OK".to_string())).with_status_code(200))
//                     } else {
//                         info!("Call cleanup contract offers feature disabled.");
//                         Response::json(&("Disabled".to_string())).with_status_code(400)
//                     }
//                 },
//                 (GET) (/health) => {
//                     Response::json(&("OK".to_string())).with_status_code(200)
//                 },
//                 // (GET) (/unlockutxos) => {
//                 //     unlock_utxos(wallet2.clone(), Response::json(&("OK".to_string())).with_status_code(200))
//                 // },
//                 // (GET) (/empty_to_address/{address: String}) => {
//                 //     empty_to_address(address, wallet2.clone(), Response::json(&("OK".to_string())).with_status_code(200))
//                 // },
//                 (GET) (/info) => {
//                     info!("Call info.");
//                     add_access_control_headers(get_wallet_info(manager.clone(), wallet.clone(), static_address.to_string()))
//                 },
//                 (POST) (/offer) => {
//                     info!("Call POST (create) offer {:?}", request);
//                     #[derive(Deserialize)]
//                     #[serde(rename_all = "camelCase")]
//                     struct OfferRequest {
//                         uuid: String,
//                         accept_collateral: u64,
//                         offer_collateral: u64,
//                         total_outcomes: u64,
//                         attestor_list: String
//                     }

//                     let req: OfferRequest = try_or_400!(rouille::input::json_input(request));

//                             // Set up Oracle Clients
//                     let bitcoin_contract_attestor_urls: Vec<String> = match serde_json::from_str(&req.attestor_list.clone()) {
//                         Ok(vec) => vec,
//                         Err(e) => {
//                             eprintln!("Error deserializing Attestor URLs: {}", e);
//                             Vec::new()
//                         }
//                     };

//                     let bitcoin_contract_attestors: HashMap<XOnlyPublicKey, Arc<P2PDOracleClient>> = generate_p2pd_clients(bitcoin_contract_attestor_urls.clone());

//                     add_access_control_headers(create_new_offer(manager.clone(), bitcoin_contract_attestors.values().cloned().collect(), active_network, req.uuid, req.accept_collateral, req.offer_collateral, req.total_outcomes))
//                 },
//                 (OPTIONS) (/offer) => {
//                     add_access_control_headers(Response::empty_204())
//                 },
//                 (OPTIONS) (/offer/accept) => {
//                     add_access_control_headers(Response::empty_204())
//                 },
//                 (PUT) (/offer/accept) => {
//                     info!("Call PUT (accept) offer {:?}", request);
//                     #[derive(Deserialize)]
//                     #[serde(rename_all = "camelCase")]
//                     struct AcceptOfferRequest {
//                         accept_message: String,
//                     }
//                     let json: AcceptOfferRequest = try_or_400!(rouille::input::json_input(request));
//                     let accept_dlc: AcceptDlc = match serde_json::from_str(&json.accept_message)
//                     {
//                         Ok(dlc) => dlc,
//                         Err(e) => return add_access_control_headers(Response::json(&ErrorsResponse{status: 400, errors: vec![ErrorResponse{message: e.to_string(), code: None}]}).with_status_code(400)),
//                     };
//                     accept_offer(accept_dlc, manager.clone())
//                 },
//                 _ => rouille::Response::empty_404()
//         )
//     });

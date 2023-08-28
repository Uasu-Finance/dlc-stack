// #![deny(warnings)]
#![feature(async_fn_in_trait)]
#![allow(unreachable_code)]

use bdk::keys::DerivableKey;
use bitcoin::util::bip32::{DerivationPath, ExtendedPrivKey};
use bytes::Buf;
use tokio::sync::oneshot;

use hyper::service::{make_service_fn, service_fn};
use hyper::Error;
use hyper::{header, Body, Method, Response, Server, StatusCode};
use url::form_urlencoded;

extern crate pretty_env_logger;

extern crate log;

use bdk::wallet::AddressIndex;
use bdk::{descriptor, FeeRate, SyncOptions};
use bdk::{SignOptions, Wallet as BdkWallet};
use serde::{Deserialize, Serialize};

use core::panic;
use std::{
    collections::HashMap,
    env,
    str::FromStr,
    sync::{Arc, Mutex},
};

use bitcoin::{Address, XOnlyPublicKey};
use dlc_bdk_wallet::DlcBdkWallet;
// use dlc_link_manager::Manager;
use dlc_link_manager::{AsyncOracle, AsyncStorage, Manager};
use dlc_manager::{
    contract::{
        contract_input::{ContractInput, ContractInputInfo, OracleInput},
        Contract,
    },
    // manager::Manager,
    SystemTimeProvider,
};
use dlc_messages::{AcceptDlc, Message};
// use electrs_blockchain_provider::ElectrsBlockchainProvider;
use esplora_async_blockchain_provider::EsploraAsyncBlockchainProvider;
use log::{debug, error, info, warn};

// use crate::storage::storage_provider::StorageProvider;
use attestor_client::AttestorClient;
use serde_json::json;
use std::fmt::{self, Write as _};
use storage::async_storage_api::AsyncStorageApiProvider;
use utils::get_numerical_contract_info;

mod storage;
mod utils;
#[macro_use]
mod macros;

type GenericError = Box<dyn std::error::Error + Send + Sync>;
#[derive(Debug)]
struct WalletError(String);
impl fmt::Display for WalletError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Wallet Error: {}", self.0)
    }
}
impl std::error::Error for WalletError {}
static NOTFOUND: &[u8] = b"Not Found";
// remove lifetime?
type DlcManager<'a> = Manager<
    Arc<DlcBdkWallet>,
    Arc<EsploraAsyncBlockchainProvider>,
    Arc<AsyncStorageApiProvider>,
    Arc<AttestorClient>,
    Arc<SystemTimeProvider>,
    // Arc<EsploraAsyncBlockchainProvider>,
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

fn main() {
    pretty_env_logger::init();

    // Configure a runtime that runs everything on the current thread
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build runtime");

    // Combine it with a `LocalSet,  which means it can spawn !Send futures...
    let local = tokio::task::LocalSet::new();
    local.block_on(&rt, run());
}

fn build_success_response(message: String) -> Result<Response<Body>, GenericError> {
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(message.to_string()))
        .unwrap())
}

fn build_error_response(message: String) -> Result<Response<Body>, GenericError> {
    Ok(Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!(
                {
                    "status": 400,
                    "errors": vec![ErrorResponse {
                        message: message.to_string(),
                        code: None,
                    }],
                }
            )
            .to_string(),
        ))?)
}

async fn run() {
    let wallet_backend_port: String = env::var("WALLET_BACKEND_PORT").unwrap_or("8085".to_string());
    let xpriv_str = env::var("XPRIV")
        .expect("XPRIV environment variable not set, please run `just generate-descriptor`, securely backup the output, and set this env_var accordingly");
    let xpriv = ExtendedPrivKey::from_str(&xpriv_str).expect("Unable to decode xpriv env variable");
    let fingerprint = env::var("FINGERPRINT")
        .expect("FINGERPRINT environment variable not set, please run `just generate-descriptor`, securely backup the output, and set this env_var accordingly");

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

    let secp = bitcoin::secp256k1::Secp256k1::new();

    // Set up wallet store
    let root_sled_path: String = env::var("SLED_WALLET_PATH").unwrap_or("wallet_db".to_string());
    let sled_path = format!("{root_sled_path}_{active_network}_{fingerprint}");
    let sled = sled::open(sled_path)
        .unwrap()
        .open_tree("default_tree")
        .unwrap();

    let attestor_urls: Vec<String> = get_attestors().await.unwrap();

    let blockchain_interface_url = env::var("BLOCKCHAIN_INTERFACE_URL")
        .expect("BLOCKCHAIN_INTERFACE_URL environment variable not set, couldn't get attestors");

    let funded_endpoint_url = format!("{}/set-status-funded", blockchain_interface_url);
    let closed_endpoint_url = format!("{}/post-close-dlc", blockchain_interface_url);

    let funded_uuids: Box<Vec<String>> = Box::new(vec![]);
    let closed_uuids: Box<Vec<String>> = Box::new(vec![]);

    // ELECTRUM / ELECTRS
    let electrs_host =
        env::var("ELECTRUM_API_URL").unwrap_or("https://blockstream.info/testnet/api/".to_string());
    let blockchain = Arc::new(EsploraAsyncBlockchainProvider::new(
        electrs_host.to_string(),
        active_network,
    ));

    let ext_path = DerivationPath::from_str("m/86h/0h/0h/0").expect("A valid derivation path");
    let int_path = DerivationPath::from_str("m/86h/0h/0h/1").expect("A valid derivation path");

    let derived_ext_pkey = xpriv.derive_priv(&secp, &ext_path).unwrap();
    let seckey_ext = derived_ext_pkey.private_key;

    let derived_int_pkey = xpriv.derive_priv(&secp, &int_path).unwrap();
    // let seckey_int = derived_int_pkey.private_key;

    let pubkey_ext = seckey_ext.public_key(&secp);
    // let pubkey_int = seckey_int.public_key(&secp);

    let signing_external_descriptor = descriptor!(wpkh(
        derived_ext_pkey
            .into_descriptor_key(
                Some((derived_ext_pkey.fingerprint(&secp), ext_path.clone())),
                ext_path
            )
            .unwrap()
    ))
    .unwrap();
    let signing_internal_descriptor = descriptor!(wpkh(
        derived_int_pkey
            .into_descriptor_key(
                Some((derived_int_pkey.fingerprint(&secp), int_path.clone())),
                int_path
            )
            .unwrap()
    ))
    .unwrap();

    let bdk_wallet = Arc::new(Mutex::new(
        BdkWallet::new(
            signing_external_descriptor,
            Some(signing_internal_descriptor),
            active_network,
            sled,
        )
        .unwrap(),
    ));

    let static_address = bdk_wallet
        .lock()
        .unwrap()
        .get_address(AddressIndex::Peek(0))
        .unwrap();
    println!("Address: {}", static_address);

    let wallet: Arc<DlcBdkWallet> = Arc::new(DlcBdkWallet::new(
        bdk_wallet,
        static_address.clone(),
        seckey_ext.clone(),
        active_network,
    ));

    // Set up Oracle Client
    let protocol_wallet_attestors = generate_attestor_client(attestor_urls.clone()).await;

    // retry!(
    // blockchain.get_blockchain_height(),
    //     10,
    //     "get blockchain height"
    // );

    // Set up DLC store
    let dlc_store = Arc::new(AsyncStorageApiProvider::new(
        pubkey_ext.to_string(),
        "https://devnet.dlc.link/storage-api".to_string(),
    ));

    // Set up time provider
    let time_provider = SystemTimeProvider {};
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

    let make_service = make_service_fn(move |_| {
        // For each connection, clone the counter to use in our service...
        let manager = manager.clone();
        let blockchain = blockchain.clone();
        let dlc_store = dlc_store.clone();
        let wallet = wallet.clone();
        let funded_endpoint_url = funded_endpoint_url.clone();
        let funded_uuids = funded_uuids.clone();
        let closed_endpoint_url = closed_endpoint_url.clone();
        let closed_uuids = closed_uuids.clone();

        async move {
            Ok::<_, Error>(service_fn(move |req| {
                let manager = manager.clone();
                let blockchain = blockchain.clone();
                let dlc_store = dlc_store.clone();
                let wallet = wallet.clone();
                let funded_endpoint_url = funded_endpoint_url.clone();
                let mut funded_uuids = funded_uuids.clone();
                let closed_endpoint_url = closed_endpoint_url.clone();
                let mut closed_uuids = closed_uuids.clone();
                async move {
                    match (req.method(), req.uri().path()) {
                        // (&Method::GET, "/cleanup") => {
                        //     let contract_cleanup_enabled: bool =
                        //         env::var("CONTRACT_CLEANUP_ENABLED")
                        //             .unwrap_or("false".to_string())
                        //             .parse()
                        //             .unwrap_or(false);
                        //     if contract_cleanup_enabled {
                        //         info!("Call cleanup contract offers.");
                        //         delete_all_offers(manager)
                        //     } else {
                        //         info!("Call cleanup contract offers feature disabled.");
                        //         return Ok(Response::builder()
                        //             .status(StatusCode::BAD_REQUEST)
                        //             .header(header::CONTENT_TYPE, "application/json")
                        //             .body(Body::from(
                        //                 json!(
                        //                     {
                        //                         "status": 400,
                        //                         "errors": vec![ErrorResponse {
                        //                             message: "Feature disabled".to_string(),
                        //                             code: None,
                        //                         }],
                        //                     }
                        //                 )
                        //                 .to_string(),
                        //             ))?);
                        //     }
                        // }
                        (&Method::GET, "/empty_to_address") => {
                            let result = async {
                                let query = req.uri().query().ok_or(WalletError(
                                    "Unable to find query on Request object".to_string(),
                                ))?;
                                let params = form_urlencoded::parse(query.as_bytes())
                                    .into_owned()
                                    .collect::<HashMap<String, String>>();
                                let address = params.get("address").ok_or(WalletError(
                                    "Unable to find address in query params".to_string(),
                                ))?;
                                empty_to_address(&address, wallet, blockchain).await
                            };
                            match result.await {
                                Ok(message) => build_success_response(message),
                                Err(e) => {
                                    warn!("Error emptying to address - {}", e);
                                    build_error_response(e.to_string())
                                }
                            }
                        }
                        (&Method::GET, "/info") => get_wallet_info(dlc_store, wallet).await,
                        (&Method::GET, "/periodic_check") => {
                            // This needs to do the updates funding / post-close stuff
                            refresh_wallet(blockchain, wallet).await?;
                            match periodic_check(
                                manager,
                                dlc_store,
                                funded_endpoint_url,
                                &mut funded_uuids,
                                closed_endpoint_url,
                                &mut closed_uuids,
                            )
                            .await
                            {
                                Ok(_) => (),
                                Err(e) => {
                                    warn!("Error periodic check: {}", e.to_string());
                                    return build_error_response(e.to_string());
                                }
                            };
                            build_success_response("Periodic check complete".to_string())
                        }
                        (&Method::POST, "/offer") => {
                            #[derive(Deserialize)]
                            #[serde(rename_all = "camelCase")]
                            struct OfferRequest {
                                uuid: String,
                                accept_collateral: u64,
                                offer_collateral: u64,
                                total_outcomes: u64,
                                attestor_list: String,
                            }

                            let whole_body = hyper::body::aggregate(req).await?;

                            let req: OfferRequest =
                                serde_json::from_reader(whole_body.reader()).unwrap();

                            let bitcoin_contract_attestor_urls: Vec<String> =
                                match serde_json::from_str(&req.attestor_list.clone()) {
                                    Ok(vec) => vec,
                                    Err(e) => {
                                        error!("Error deserializing Attestor URLs: {e}",);
                                        return build_error_response(format!(
                                            "Error deserializing Attestor URLs {e}"
                                        ));
                                    }
                                };

                            let bitcoin_contract_attestors: HashMap<
                                XOnlyPublicKey,
                                Arc<AttestorClient>,
                            > = generate_attestor_client(bitcoin_contract_attestor_urls.clone())
                                .await;

                            let offer_string = create_new_offer(
                                manager,
                                bitcoin_contract_attestors,
                                active_network,
                                req.uuid,
                                req.accept_collateral,
                                req.offer_collateral,
                                req.total_outcomes,
                            )
                            .await?;
                            build_success_response(offer_string)
                        }
                        (&Method::PUT, "/offer/accept") => {
                            info!("Accepting offer");
                            // Aggregate the body...
                            let whole_body = hyper::body::aggregate(req).await?;
                            // Decode as JSON...
                            #[derive(Deserialize)]
                            #[serde(rename_all = "camelCase")]
                            struct AcceptOfferRequest {
                                accept_message: String,
                            }
                            let data: AcceptOfferRequest =
                                serde_json::from_reader(whole_body.reader()).unwrap();
                            let accept_dlc: AcceptDlc =
                                match serde_json::from_str(&data.accept_message) {
                                    Ok(data) => data,
                                    Err(e) => {
                                        error!("Error deserializing AcceptDlc object: {e}",);
                                        return build_error_response(e.to_string());
                                    }
                                };

                            let sign_string = accept_offer(accept_dlc, manager).await?;
                            build_success_response(sign_string)
                        }
                        _ => {
                            // Return 404 not found response.
                            Ok(Response::builder()
                                .status(StatusCode::NOT_FOUND)
                                .body(NOTFOUND.into())
                                .unwrap())
                        }
                    }
                }
                // response_examples(req, client.to_owned())
            }))
        }
    });

    let addr = (
        [127, 0, 0, 1],
        wallet_backend_port.parse().expect("Correct port value"),
    )
        .into();

    let server = Server::bind(&addr).executor(LocalExec).serve(make_service);

    // Just shows that with_graceful_shutdown compiles with !Send,
    // !Sync HttpBody.
    let (_tx, rx) = oneshot::channel::<()>();
    let server = server.with_graceful_shutdown(async move {
        rx.await.ok();
    });

    warn!("Listening on http://{}", addr);

    // The server would block on current thread to await !Send futures.
    if let Err(e) = server.await {
        panic!("server error: {}", e);
    }
}

async fn create_new_offer(
    manager: Arc<Mutex<DlcManager<'_>>>,
    attestors: HashMap<XOnlyPublicKey, Arc<AttestorClient>>,
    active_network: bitcoin::Network,
    event_id: String,
    accept_collateral: u64,
    offer_collateral: u64,
    total_outcomes: u64,
) -> Result<String, GenericError> {
    let (_event_descriptor, descriptor) = get_numerical_contract_info(
        accept_collateral,
        offer_collateral,
        total_outcomes,
        attestors.len(),
    );
    info!(
        "Creating new offer with event id: {}, accept collateral: {}, offer_collateral: {}",
        event_id.clone(),
        accept_collateral,
        offer_collateral
    );

    let public_keys = attestors.clone().into_keys().collect();
    let contract_info = ContractInputInfo {
        oracles: OracleInput {
            public_keys,
            event_id: event_id.clone(),
            threshold: attestors.len() as u16,
        },
        contract_descriptor: descriptor,
    };

    for (_k, attestor) in attestors {
        // check if the oracle has an event with the id of event_id
        let _announcement = attestor.get_announcement(&event_id).await?;
    }

    // Some regtest networks have an unreliable fee estimation service
    let fee_rate = match active_network {
        bitcoin::Network::Regtest => 1,
        _ => 400,
    };

    println!("contract_info: {:?}", contract_info);

    let contract_input = ContractInput {
        offer_collateral,
        accept_collateral,
        fee_rate,
        contract_infos: vec![contract_info],
    };

    //had to make this mutable because of the borrow, not sure why
    let mut man = manager.lock().unwrap();

    let offer = man
        .send_offer(
            &contract_input,
            STATIC_COUNTERPARTY_NODE_ID.parse().unwrap(),
        )
        .await?;
    serde_json::to_string(&offer).map_err(|e| e.into())
}

async fn accept_offer(
    accept_dlc: AcceptDlc,
    manager: Arc<Mutex<DlcManager<'_>>>,
) -> Result<String, GenericError> {
    debug!("accept_dlc: {:?}", accept_dlc);

    let dlc = manager
        .lock()
        .unwrap()
        .on_dlc_message(
            &Message::Accept(accept_dlc),
            STATIC_COUNTERPARTY_NODE_ID.parse().unwrap(),
        )
        .await?;

    match dlc {
        Some(Message::Sign(sign)) => serde_json::to_string(&sign).map_err(|e| e.into()),
        _ => Err("Error: invalid Sign message for accept_offer function".into()),
    }
}

async fn get_wallet_info(
    store: Arc<AsyncStorageApiProvider>,
    wallet: Arc<DlcBdkWallet>,
    // static_address: String,
) -> Result<Response<Body>, GenericError> {
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
    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(info_response.to_string()))?;
    Ok(response)
}

async fn refresh_wallet(
    blockchain: Arc<EsploraAsyncBlockchainProvider>,
    wallet: Arc<DlcBdkWallet>,
) -> Result<(), WalletError> {
    let bdk = match wallet.bdk_wallet.lock() {
        Ok(wallet) => wallet,
        Err(e) => {
            error!("Error locking wallet: {}", e.to_string());
            return Err(WalletError(e.to_string()));
        }
    };

    bdk.sync(&blockchain.blockchain, SyncOptions::default())
        .await
        .map_err(|e| WalletError(e.to_string()))?;

    Ok(())
}

async fn periodic_check(
    manager: Arc<Mutex<DlcManager<'_>>>,
    store: Arc<AsyncStorageApiProvider>,
    funded_url: String,
    funded_uuids: &mut Vec<String>,
    closed_url: String,
    closed_uuids: &mut Vec<String>,
) -> Result<String, GenericError> {
    debug!("Running periodic_check");

    // This should ideally not be done as a mutable ref as it could cause a runtime error
    // when you have a reference to an object as mut and not mut at the same time
    let mut man = manager.lock().unwrap();

    let updated_contracts = match man.periodic_check().await {
        Ok(updated_contracts) => updated_contracts,
        Err(e) => {
            info!("Error in periodic_check, will retry: {}", e.to_string());
            vec![]
        }
    };
    let mut newly_confirmed_uuids: Vec<String> = vec![];
    let mut newly_closed_uuids: Vec<(String, bitcoin::Txid)> = vec![];

    for (id, uuid) in updated_contracts {
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

        match contract {
            Contract::Confirmed(_c) => {
                newly_confirmed_uuids.push(uuid);
            }
            Contract::Closed(c) => {
                newly_closed_uuids.push((uuid, c.signed_cet.unwrap().txid()));
            }
            _ => error!(
                "Error retrieving contract in periodic_check: {:?}, skipping",
                id
            ),
        };
    }

    for uuid in newly_confirmed_uuids {
        if !funded_uuids.contains(&uuid) {
            debug!("Contract is funded, setting funded to true: {}", uuid);
            reqwest::Client::new()
                .post(&funded_url)
                .json(&json!({ "uuid": uuid }))
                .send()
                .await?;
        }
    }

    for (uuid, txid) in newly_closed_uuids {
        if !closed_uuids.contains(&uuid) {
            debug!("Contract is closed, firing post-close url: {}", uuid);
            reqwest::Client::new()
                .post(&closed_url)
                .json(&json!({"uuid": uuid, "btcTxId": txid.to_string()}))
                .send()
                .await?;
        }
    }
    Ok("Success running periodic check".to_string())
}

// fn delete_all_offers(manager: Arc<Mutex<DlcManager>>) -> Result<Response<Body>, GenericError> {
//     info!("Deleting all contracts from dlc-store");
//     let man = manager.lock().unwrap();
//     man.get_store().delete_contracts();
//     Ok(Response::builder()
//         .status(StatusCode::OK)
//         .body(Body::from("Success".to_string()))
//         .unwrap())
// }

async fn empty_to_address(
    address: &str,
    wallet: Arc<DlcBdkWallet>,
    blockchain: Arc<EsploraAsyncBlockchainProvider>,
) -> Result<String, WalletError> {
    let bdk = match wallet.bdk_wallet.lock() {
        Ok(wallet) => wallet,
        Err(e) => {
            error!("Error locking wallet: {}", e.to_string());
            return Err(WalletError(e.to_string()));
        }
    };

    let to_address = Address::from_str(address).map_err(|e| WalletError(e.to_string()))?;
    info!("draining wallet to address: {}", to_address);
    let mut builder = bdk.build_tx();
    builder
        .drain_wallet()
        .drain_to(to_address.script_pubkey())
        .fee_rate(FeeRate::from_sat_per_vb(5.0))
        .enable_rbf();
    let (mut psbt, _details) = builder.finish().map_err(|e| WalletError(e.to_string()))?;

    let _finalized = bdk
        .sign(&mut psbt, SignOptions::default())
        .map_err(|e| WalletError(e.to_string()))?;

    // Broadcast the transaction
    let raw_transaction = psbt.extract_tx();
    let txid = raw_transaction.txid();

    blockchain
        .blockchain
        .broadcast(&raw_transaction)
        .await
        .map_err(|e| WalletError(e.to_string()))?;
    Ok(format!("Transaction broadcast successfully, TXID: {txid}"))
}
// Since the Server needs to spawn some background tasks, we needed
// to configure an Executor that can spawn !Send futures...
#[derive(Clone, Copy, Debug)]
struct LocalExec;

impl<F> hyper::rt::Executor<F> for LocalExec
where
    F: std::future::Future + 'static, // not requiring `Send`
{
    fn execute(&self, fut: F) {
        // This will spawn into the currently running `LocalSet`.
        tokio::task::spawn_local(fut);
    }
}

// #![deny(warnings)]
#![feature(async_fn_in_trait)]
#![allow(unreachable_code)]

// use log::warn;
use bytes::Buf;
use futures_util::{stream, StreamExt};
use std::cell::Cell;
use std::rc::Rc;
use tokio::sync::oneshot;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::client::HttpConnector;
use hyper::header::{HeaderMap, HeaderValue};
use hyper::service::{make_service_fn, service_fn};
use hyper::Error;
use hyper::{header, Body, Client, Method, Request, Response, Server, StatusCode};
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::net::{TcpListener, TcpStream};

extern crate pretty_env_logger;
#[macro_use]
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

type GenericError = Box<dyn std::error::Error + Send + Sync>;
// type Result<T> = std::result::Result<T, GenericError>;
type BoxBody = http_body_util::combinators::BoxBody<Bytes, hyper::Error>;

static INDEX: &[u8] = b"<a href=\"test.html\">test.html</a>";
static INTERNAL_SERVER_ERROR: &[u8] = b"Internal Server Error";
static NOTFOUND: &[u8] = b"Not Found";
static POST_DATA: &str = r#"{"original": "data"}"#;
static URL: &str = "http://127.0.0.1:1337/json_api";

// remove lifetime?
type DlcManager<'a> = Manager<
    Arc<DlcBdkWallet>,
    Arc<EsploraAsyncBlockchainProvider>,
    Arc<AsyncStorageApiProvider>,
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

async fn client_request_response(
    client: &Client<HttpConnector>,
) -> Result<Response<Body>, GenericError> {
    let req = Request::builder()
        .method(Method::POST)
        .uri(URL)
        .header(header::CONTENT_TYPE, "application/json")
        .body(POST_DATA.into())
        .unwrap();

    let web_res = client.request(req).await?;
    // Compare the JSON we sent (before) with what we received (after):
    let before = stream::once(async {
        Ok(format!(
            "<b>POST request body</b>: {}<br><b>Response</b>: ",
            POST_DATA,
        )
        .into())
    });
    let after = web_res.into_body();
    let body = Body::wrap_stream(before.chain(after));

    Ok(Response::new(body))
}

async fn api_post_response(req: Request<Body>) -> Result<Response<Body>, GenericError> {
    // Aggregate the body...
    let whole_body = hyper::body::aggregate(req).await?;
    // Decode as JSON...
    let mut data: serde_json::Value = serde_json::from_reader(whole_body.reader())?;
    // Change the JSON...
    data["test"] = serde_json::Value::from("test_value");
    // And respond with the new JSON.
    let json = serde_json::to_string(&data)?;
    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json))?;
    Ok(response)
}

async fn api_get_response() -> Result<Response<Body>, GenericError> {
    let data = vec!["foo", "bar"];
    let res = match serde_json::to_string(&data) {
        Ok(json) => Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json))
            .unwrap(),
        Err(_) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(INTERNAL_SERVER_ERROR.into())
            .unwrap(),
    };
    Ok(res)
}
// async fn response_examples(
//     req: Request<Body>,
//     client: Client<HttpConnector>,
// ) -> Result<Response<Body>, GenericError> {
//     match (req.method(), req.uri().path()) {
//         (&Method::GET, "/") | (&Method::GET, "/index.html") => Ok(Response::new(INDEX.into())),
//         (&Method::GET, "/test.html") => client_request_response(&client).await,
//         (&Method::POST, "/json_api") => api_post_response(req).await,
//         (&Method::GET, "/json_api") => api_get_response().await,
//         (&Method::GET, "/periodic_check") => periodic_check(manager, dlc_store).await,
//         _ => {
//             // Return 404 not found response.
//             Ok(Response::builder()
//                 .status(StatusCode::NOT_FOUND)
//                 .body(NOTFOUND.into())
//                 .unwrap())
//         }
//     }
// }

async fn run() {
    let client = Client::new();
    // Using a !Send request counter is fine on 1 thread...
    let counter = Rc::new(Cell::new(0));

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

    // Set up wallet store
    let root_sled_path: String = env::var("SLED_WALLET_PATH").unwrap_or("wallet_db".to_string());
    let sled_path = format!("{root_sled_path}_{}", active_network);
    let sled = sled::open(sled_path)
        .unwrap()
        .open_tree("default_tree")
        .unwrap();

    let attestor_urls: Vec<String> = get_attestors().await.unwrap();

    let blockchain_interface_url = env::var("BLOCKCHAIN_INTERFACE_URL")
        .expect("BLOCKCHAIN_INTERFACE_URL environment variable not set, couldn't get attestors");

    let funded_endpoint_url = format!("{}/set-status-funded", blockchain_interface_url);

    let mut funded_uuids: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));

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

    let wallet: Arc<DlcBdkWallet> = Arc::new(DlcBdkWallet::new(
        bdk_wallet,
        static_address.clone(),
        seckey.clone(),
        active_network,
    ));

    // Set up Oracle Client
    let protocol_wallet_attestors = generate_p2pd_clients(attestor_urls.clone()).await;

    // retry!(
    // blockchain.get_blockchain_height(),
    //     10,
    //     "get blockchain height"
    // );

    // Set up DLC store
    let dlc_store = Arc::new(AsyncStorageApiProvider::new(
        pubkey.to_string(),
        "https://devnet.dlc.link/storage-api".to_string(),
    ));

    // Create the DLC Manager

    // let addr = ([127, 0, 0, 1], 3000).into();

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
        let cnt = counter.clone();
        let manager = manager.clone();
        let dlc_store = dlc_store.clone();
        let client = client.clone();
        let wallet = wallet.clone();

        async move {
            Ok::<_, Error>(service_fn(move |req| {
                let prev = cnt.get();
                cnt.set(prev + 1);
                let value = cnt.get();
                let manager = manager.clone();
                let dlc_store = dlc_store.clone();
                let wallet = wallet.clone();
                let client = client.clone();
                async move {
                    // let resp = periodic_check(manager.clone(), dlc_store.clone())
                    //     .await
                    //     .unwrap();
                    // info!("resp: {:?}", resp);

                    // Ok::<_, Error>(Response::new(Body::from(format!("Request #{}", value))))
                    // Ok::<_, GenericError>(service_fn(move |req| {
                    // Clone again to ensure that client outlives this closure.

                    // }))
                    match (req.method(), req.uri().path()) {
                        (&Method::GET, "/") | (&Method::GET, "/index.html") => {
                            Ok(Response::new(INDEX.into()))
                        }
                        (&Method::GET, "/test.html") => client_request_response(&client).await,
                        (&Method::POST, "/json_api") => api_post_response(req).await,
                        (&Method::GET, "/json_api") => api_get_response().await,
                        (&Method::GET, "/info") => get_wallet_info(dlc_store, wallet).await,
                        (&Method::GET, "/periodic_check") => {
                            periodic_check(manager, dlc_store).await
                            //update funded uuids code goes here
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

    let addr = ([127, 0, 0, 1], 3000).into();

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
        eprintln!("server error: {}", e);
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

async fn periodic_check(
    manager: Arc<Mutex<DlcManager<'_>>>,
    store: Arc<AsyncStorageApiProvider>,
) -> Result<Response<Body>, GenericError> {
    let mut man = manager.lock().unwrap();

    let updated_contract_ids = match man.periodic_check().await {
        Ok(updated_contract_ids) => updated_contract_ids,
        Err(e) => {
            info!("Error in periodic_check, will retry: {}", e.to_string());
            vec![]
        }
    };
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
    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(format!("{:?}", newly_confirmed_uuids)))?;
    Ok(response)
    // Ok(newly_confirmed_uuids)
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

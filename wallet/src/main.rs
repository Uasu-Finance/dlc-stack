// #![deny(warnings)]
#![feature(async_fn_in_trait)]
#![allow(unreachable_code)]

use bdk::blockchain::{self, Progress};
// use log::warn;
use bdk::wallet::AddressIndex::New;
use bytes::Buf;
use dlc_manager::Wallet;
use futures_util::{stream, StreamExt};
use tokio::sync::oneshot;

use hyper::body::Bytes;
use hyper::client::HttpConnector;
use hyper::service::{make_service_fn, service_fn};
use hyper::Error;
use hyper::{header, Body, Client, Method, Request, Response, Server, StatusCode};
use url::form_urlencoded;

extern crate pretty_env_logger;
#[macro_use]
extern crate log;

use bdk::{descriptor::IntoWalletDescriptor, wallet::AddressIndex};
use bdk::{FeeRate, SyncOptions};
use bdk::{SignOptions, Wallet as BdkWallet};
use serde::{Deserialize, Serialize};

use std::{
    collections::HashMap,
    env,
    str::FromStr,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use bitcoin::{Address, KeyPair, XOnlyPublicKey};
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
    Arc<AttestorClient>,
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

async fn process_post_params(req: Request<Body>) -> Result<Response<Body>, GenericError> {
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

async fn process_get_params() -> Result<Response<Body>, GenericError> {
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

async fn run() {
    let client = Client::new();

    let wallet_backend_port: String = env::var("WALLET_BACKEND_PORT").unwrap_or("8085".to_string());
    let wallet_descriptor_string = env::var("WALLET_DESCRIPTOR")
        .expect("WALLET_DESCRIPTOR environment variable not set, please run `just generate-descriptor`, securely backup the output, and set this env_var accordingly");
    let wallet_pkey = env::var("WALLET_PKEY")
        .expect("WALLET_PKEY environment variable not set, please run `just generate-descriptor`, securely backup the output, and set this env_var accordingly");

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
    let (wallet_desc, keymap) = wallet_descriptor_string
        .into_wallet_descriptor(&secp, active_network)
        .unwrap();

    println!("wallet_desc: {:?}", wallet_desc);
    println!("\n\nkeymap: {:?}", keymap);
    let x_pub_key = keymap.keys().next().unwrap();
    let x_priv_key = keymap.get(x_pub_key).unwrap();
    println!("x_pub_key: {:?}", x_pub_key);
    println!("x_priv_key: {:?}", x_priv_key);

    // match x_priv_key {
    // miniscript::::Single(skey) => println!("SinglePriv"),
    // /// Extended private key (xpriv).
    // XPrv(xkey) => println!("XPrv"),
    // }

    //How to get a derived private_key from the keymap and xprivatekey of the descriptor

    // this is creating a 66 hex-character pubkey, but in attestor we are currently creating an xpubkey with only 64 characters
    let pubkey = x_pub_key
        .clone()
        .at_derivation_index(0)
        .derive_public_key(&secp)
        .unwrap()
        .inner
        .to_string();

    let keypair = KeyPair::from_seckey_str(&secp, &wallet_pkey).unwrap();

    let seckey = keypair.secret_key();

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
        BdkWallet::new(wallet_desc, None, active_network, sled).unwrap(),
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
    let protocol_wallet_attestors = generate_attestor_client(attestor_urls.clone()).await;

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
        let manager = manager.clone();
        let blockchain = blockchain.clone();
        let dlc_store = dlc_store.clone();
        let wallet = wallet.clone();

        async move {
            Ok::<_, Error>(service_fn(move |req| {
                let manager = manager.clone();
                let blockchain = blockchain.clone();
                let dlc_store = dlc_store.clone();
                let wallet = wallet.clone();
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
                                Ok(message) => Ok(Response::builder()
                                    .status(StatusCode::OK)
                                    .header(header::CONTENT_TYPE, "application/json")
                                    .body(Body::from(message.to_string()))
                                    .unwrap()),
                                Err(e) => {
                                    warn!("Error emptying to address - {}", e);
                                    return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .header(header::CONTENT_TYPE, "application/json")
                                        .body(Body::from(
                                            json!(
                                                {
                                                    "status": 400,
                                                    "errors": vec![ErrorResponse {
                                                        message: e.to_string(),
                                                        code: None,
                                                    }],
                                                }
                                            )
                                            .to_string(),
                                        ))?);
                                }
                            }
                        }
                        (&Method::GET, "/info") => get_wallet_info(dlc_store, wallet).await,
                        (&Method::GET, "/periodic_check") => {
                            // This needs to do the updates funding / post-close stuff
                            if refresh_wallet(blockchain, wallet).await.is_err() {
                                warn!("Error refreshing wallet: ");
                                return Ok(Response::builder()
                                    .status(StatusCode::BAD_REQUEST)
                                    .header(header::CONTENT_TYPE, "application/json")
                                    .body(Body::from(
                                        json!(
                                            {
                                                "status": 400,
                                                "errors": vec![ErrorResponse {
                                                    message: "Error refreshing wallet".to_string(),
                                                    code: None,
                                                }],
                                            }
                                        )
                                        .to_string(),
                                    ))?);
                            };
                            periodic_check(manager, dlc_store).await
                            //update funded uuids code goes here
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
                                        eprintln!("Error deserializing Attestor URLs: {}", e);
                                        Vec::new()
                                    }
                                };

                            let bitcoin_contract_attestors: HashMap<
                                XOnlyPublicKey,
                                Arc<AttestorClient>,
                            > = generate_attestor_client(bitcoin_contract_attestor_urls.clone())
                                .await;

                            create_new_offer(
                                manager,
                                bitcoin_contract_attestors,
                                active_network,
                                req.uuid,
                                req.accept_collateral,
                                req.offer_collateral,
                                req.total_outcomes,
                            )
                            .await
                            //update funded uuids code goes here
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
                                        return Ok(Response::builder()
                                            .status(StatusCode::BAD_REQUEST)
                                            .header(header::CONTENT_TYPE, "application/json")
                                            .body(Body::from(
                                                json!(
                                                    {
                                                        "status": 400,
                                                        "errors": vec![ErrorResponse {
                                                            message: e.to_string(),
                                                            code: None,
                                                        }],
                                                    }
                                                )
                                                .to_string(),
                                            ))?);
                                    }
                                };

                            accept_offer(accept_dlc, manager).await
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
        eprintln!("server error: {}", e);
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
) -> Result<Response<Body>, GenericError> {
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
        match attestor.get_announcement(&event_id).await {
            Ok(_announcement) => (),
            Err(e) => {
                info!("Error getting announcement: {}", event_id);
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!(
                           {
                                "status": 400,
                                "errors": vec![ErrorResponse {
                                    message: format!(
                                        "Error: unable to get announcement. Does it exist? -- {}",
                                        e.to_string()
                                    ),
                                    code: None,
                                }],
                            }
                        )
                        .to_string(),
                    ))?);
            }
        }
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

    match man
        .send_offer(
            &contract_input,
            STATIC_COUNTERPARTY_NODE_ID.parse().unwrap(),
        )
        .await
    {
        Ok(dlc) => Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_string(&dlc)?))?),
        Err(e) => {
            info!("DLC manager - send offer error: {}", e.to_string());
            Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "status": 400,
                        "errors": vec![ErrorResponse {
                            message: e.to_string(),
                            code: None,
                        }],
                    })
                    .to_string(),
                ))?)
        }
    }
}

async fn accept_offer(
    accept_dlc: AcceptDlc,
    manager: Arc<Mutex<DlcManager<'_>>>,
) -> Result<Response<Body>, GenericError> {
    println!("accept_dlc: {:?}", accept_dlc);
    if let Some(Message::Sign(sign)) = match manager
        .lock()
        .unwrap()
        .on_dlc_message(
            &Message::Accept(accept_dlc),
            STATIC_COUNTERPARTY_NODE_ID.parse().unwrap(),
        )
        .await
    {
        Ok(dlc) => dlc,
        Err(e) => {
            info!("DLC manager - accept offer error: {}", e.to_string());
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "status": 400,
                        "errors": vec![ErrorResponse {
                            message: e.to_string(),
                            code: None,
                        }],
                    })
                    .to_string(),
                ))?);
        }
    } {
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_string(&sign)?))?);
    } else {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "status": 400,
                    "errors": vec![ErrorResponse {
                        message: format!(
                            "Error: invalid Sign message for accept_offer function",
                        ),
                        code: None,
                    }],
                })
                .to_string(),
            ))?);
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
) -> Result<Response<Body>, GenericError> {
    let wallet = match wallet.bdk_wallet.lock() {
        Ok(wallet) => wallet,
        Err(e) => {
            error!("Error locking wallet: {}", e.to_string());
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!(
                        {
                            "status": 400,
                            "errors": vec![ErrorResponse {
                                message: e.to_string(),
                                code: None,
                            }],
                        }
                    )
                    .to_string(),
                ))?);
        }
    };

    // This doesn't work
    let progress =
        Some(Box::new(delay_progress()) as Box<(dyn bdk::blockchain::Progress + 'static)>);
    let sync_options = SyncOptions { progress };

    match wallet.sync(&blockchain.blockchain, sync_options).await {
        Ok(_) => (),
        Err(e) => {
            error!("Error syncing wallet: {}", e.to_string());
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!(
                        {
                            "status": 400,
                            "errors": vec![ErrorResponse {
                                message: e.to_string(),
                                code: None,
                            }],
                        }
                    )
                    .to_string(),
                ))?);
        }
    };

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("Refreshed wallet"))?;
    Ok(response)
}

/// Type that implements [`Progress`] and logs at level `INFO` every update received
#[derive(Clone, Copy, Default, Debug)]
pub struct DelayProgress;

/// Create a new instance of [`DelayProgress`]
pub fn delay_progress() -> DelayProgress {
    DelayProgress
}

impl Progress for DelayProgress {
    fn update(&self, progress: f32, message: Option<String>) -> Result<(), bdk::Error> {
        println!(
            "Super Sync {:.3}%: `{}`",
            progress,
            message.unwrap_or_else(|| "".into())
        );

        // Sleep for a bit to simulate a slow sync
        thread::sleep(Duration::from_millis(1000));

        Ok(())
    }
}

async fn periodic_check(
    manager: Arc<Mutex<DlcManager<'_>>>,
    store: Arc<AsyncStorageApiProvider>,
) -> Result<Response<Body>, GenericError> {
    debug!("Running periodic_check");

    // This should ideally not be done as a mutable ref as it could cause a runtime error
    // when you have a reference to an object as mut and not mut at the same time
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
    info!("Emptying to address");
    let bdk = match wallet.bdk_wallet.lock() {
        Ok(wallet) => wallet,
        Err(e) => {
            error!("Error locking wallet: {}", e.to_string());
            return Err(WalletError(e.to_string()));
        }
    };

    let mut builder = bdk.build_tx();
    builder
        .add_recipient(
            Address::from_str(address)
                .map_err(|e| WalletError(e.to_string()))?
                .script_pubkey(),
            50_000,
        )
        .enable_rbf()
        .do_not_spend_change()
        .fee_rate(FeeRate::from_sat_per_vb(5.0));
    let (mut psbt, details) = builder.finish().map_err(|e| WalletError(e.to_string()))?;

    info!("Transaction details: {:#?}", details);
    info!("Unsigned PSBT: {}", psbt);

    let finalized = bdk
        .sign(&mut psbt, SignOptions::default())
        .map_err(|e| WalletError(e.to_string()))?;
    assert!(finalized, "The PSBT was not finalized!");
    info!("The PSBT has been signed and finalized.");

    // Broadcast the transaction
    let raw_transaction = psbt.extract_tx();
    let txid = raw_transaction.txid();

    blockchain
        .blockchain
        .broadcast(&raw_transaction)
        .await
        .map_err(|e| WalletError(e.to_string()))?;
    Ok(format!("Transaction broadcast! TXID: {txid}.\nExplorer URL: https://mempool.space/testnet/tx/{txid}", txid = txid))
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

#![feature(async_fn_in_trait)]
#![deny(clippy::unwrap_used)]
#![deny(unused_mut)]
#![deny(dead_code)]
//! #Manager a component to create and update DLCs.

extern crate dlc_manager;

use crate::dlc_manager::contract::{
    accepted_contract::AcceptedContract, contract_info::ContractInfo,
    contract_input::ContractInput, offered_contract::OfferedContract,
    signed_contract::SignedContract, AdaptorInfo, ClosedContract, Contract, FailedAcceptContract,
    FailedSignContract, PreClosedContract,
};
use crate::dlc_manager::contract_updater::{accept_contract, verify_accepted_and_sign_contract};
use crate::dlc_manager::error::Error;
use crate::dlc_manager::{Blockchain, Time, Wallet};

use bitcoin::{Address, Transaction, Txid};

use dlc_manager::ContractId;
use dlc_messages::oracle_msgs::{OracleAnnouncement, OracleAttestation};
use dlc_messages::{AcceptDlc, Message as DlcMessage, OfferDlc, SignDlc};

use log::*;
use secp256k1_zkp::XOnlyPublicKey;
use secp256k1_zkp::{All, PublicKey, Secp256k1};
use std::collections::HashMap;
use std::ops::Deref;
use std::string::ToString;

/// The number of confirmations required before moving the the confirmed state.
pub const NB_CONFIRMATIONS: u32 = 6;
/// The upper bound for the delay refund verification check, 10 years.
pub const FIFTY_YEARS: u32 = 86400 * 365 * 50;
pub const ONE_DAY_IN_SECONDS: u32 = 86400;
/// The nSequence value used for CETs in DLC channels
pub const CET_NSEQUENCE: u32 = 288;
/// Timeout in seconds when waiting for a peer's reply, after which a DLC channel
/// is forced closed.
pub const PEER_TIMEOUT: u64 = 3600;

type ClosableContractInfo<'a> = Option<(
    &'a ContractInfo,
    &'a AdaptorInfo,
    Vec<(usize, OracleAttestation)>,
)>;

/// Oracle trait provides access to oracle information.
pub trait AsyncOracle {
    /// Returns the public key of the oracle.
    async fn get_public_key(&self) -> XOnlyPublicKey;
    /// Returns the announcement for the event with the given id if found.
    async fn get_announcement(&self, event_id: &str) -> Result<OracleAnnouncement, Error>;
    /// Returns the attestation for the event with the given id if found.
    async fn get_attestation(&self, event_id: &str) -> Result<OracleAttestation, Error>;
}

pub trait AsyncStorage {
    /// Returns the contract with given id if found.
    async fn get_contract(&self, id: &ContractId) -> Result<Option<Contract>, Error>;
    /// Return all contracts
    async fn get_contracts(&self) -> Result<Vec<Contract>, Error>;
    /// Create a record for the given contract.
    async fn create_contract(&self, contract: &OfferedContract) -> Result<(), Error>;
    /// Delete the record for the contract with the given id.
    async fn delete_contract(&self, id: &ContractId) -> Result<(), Error>;
    /// Update the given contract.
    async fn update_contract(&self, contract: &Contract) -> Result<(), Error>;
    /// Returns the set of contracts in offered state.
    async fn get_contract_offers(&self) -> Result<Vec<OfferedContract>, Error>;
    /// Returns the set of contracts in signed state.
    async fn get_signed_contracts(&self) -> Result<Vec<SignedContract>, Error>;
    /// Returns the set of confirmed contracts.
    async fn get_confirmed_contracts(&self) -> Result<Vec<SignedContract>, Error>;
    /// Returns the set of contracts whos broadcasted cet has not been verified to be confirmed on
    /// blockchain
    async fn get_preclosed_contracts(&self) -> Result<Vec<PreClosedContract>, Error>;
}

pub trait AsyncBlockchain {
    async fn get_transaction_confirmations_async(&self, txid: &bitcoin::Txid)
        -> Result<u32, Error>;

    async fn send_transaction_async(&self, tx: &Transaction) -> Result<(), Error>;

    async fn get_network_async(&self) -> Result<bitcoin::Network, Error>;

    async fn get_transaction_async(&self, tx_id: &Txid) -> Result<Transaction, Error>;
}

fn calculate_denominator_from_basis_points(basis_points: u64) -> u64 {
    if basis_points == 0 {
        return 0;
    }
    ((100.0 / basis_points as f64) * 100.0) as u64
}

/// Used to create and update DLCs.
pub struct Manager<W: Deref, B: Deref, S: Deref, O: Deref, T: Deref>
where
    W::Target: Wallet,
    B::Target: Blockchain + AsyncBlockchain,
    S::Target: AsyncStorage,
    O::Target: AsyncOracle,
    T::Target: Time,
{
    pub oracles: Option<HashMap<XOnlyPublicKey, O>>,
    wallet: W,
    blockchain: B,
    store: S,
    secp: Secp256k1<All>,
    time: T,
}

macro_rules! get_object_in_state {
    ($manager: ident, $id: expr, $state: ident, $peer_id: expr, $object_type: ident, $get_call: ident) => {{
        let object = $manager.store.$get_call($id).await?;
        match object {
            Some(c) => {
                if let Some(p) = $peer_id as Option<PublicKey> {
                    if c.get_counter_party_id() != p {
                        return Err(Error::InvalidParameters(format!(
                            "Peer {:02x?} is not involved with contract {:02x?}.",
                            $peer_id, $id
                        )));
                    }
                }
                match c {
                    $object_type::$state(s) => Ok(s),
                    _ => Err(Error::InvalidState(format!(
                        "Invalid state {:?} expected {}.",
                        c,
                        stringify!($state),
                    ))),
                }
            }
            None => Err(Error::InvalidParameters(format!(
                "Unknown {} id.",
                stringify!($object_type)
            ))),
        }
    }};
}

macro_rules! get_contract_in_state {
    ($manager: ident, $contract_id: expr, $state: ident, $peer_id: expr) => {{
        get_object_in_state!(
            $manager,
            $contract_id,
            $state,
            $peer_id,
            Contract,
            get_contract
        )
    }};
}

impl<W: Deref, B: Deref, S: Deref, O: Deref, T: Deref> Manager<W, B, S, O, T>
where
    W::Target: Wallet,
    B::Target: Blockchain + AsyncBlockchain,
    S::Target: AsyncStorage,
    O::Target: AsyncOracle,
    T::Target: Time,
{
    /// Create a new Manager struct.
    pub fn new(
        wallet: W,
        blockchain: B,
        store: S,
        oracles: Option<HashMap<XOnlyPublicKey, O>>,
        time: T,
    ) -> Result<Self, Error> {
        Ok(Manager {
            secp: secp256k1_zkp::Secp256k1::new(),
            wallet,
            blockchain,
            store,
            oracles,
            time,
        })
    }

    /// Get the store from the Manager to access contracts.
    pub fn get_store(&self) -> &S {
        &self.store
    }

    #[doc(hidden)]
    pub fn get_mut_store(&self) -> &S {
        &self.store
    }

    /// Function called to pass a DlcMessage to the Manager.
    pub async fn on_dlc_message(
        &self,
        msg: &DlcMessage,
        counter_party: PublicKey,
    ) -> Result<Option<DlcMessage>, Error> {
        match msg {
            DlcMessage::Offer(o) => {
                self.on_offer_message(o, counter_party).await?;
                Ok(None)
            }
            DlcMessage::Accept(a) => Ok(Some(self.on_accept_message(a, &counter_party).await?)),
            DlcMessage::Sign(s) => {
                self.on_sign_message(s, &counter_party).await?;
                Ok(None)
            }
            _ => Err(Error::InvalidState("Invalid message type.".to_string())),
        }
    }

    /// Function called to create a new DLC. The offered contract will be stored
    /// and an OfferDlc message returned.
    pub async fn send_offer(
        &self,
        contract_input: &ContractInput,
        counter_party: PublicKey,
        refund_delay: u32,
        protocol_fee_basis_points: u64,
        fee_address: Address,
    ) -> Result<OfferDlc, Error> {
        let manager_oracles = match &self.oracles {
            // Oracles is now an optional field, so check here before continuing.
            Some(oracles) => oracles,
            None => {
                return Err(Error::InvalidParameters(
                    "Manager instantiated without oracles, send_offer function not supported"
                        .to_string(),
                ));
            }
        };
        contract_input.validate()?;

        // let contract_infos = &contract_input.contract_infos;
        // let mut oracle_announcements = Vec::new();
        let event_id = &contract_input
            .contract_infos
            .first()
            .ok_or(Error::InvalidParameters(
                "Contract Input Info missing".to_string(),
            ))?
            .oracles
            .event_id;
        debug!(
            "manager_oracles keys {:?}",
            manager_oracles
                .keys()
                .map(|k| k.to_string())
                .collect::<Vec<String>>()
                .join(", ")
        );
        let oracle_set: Vec<Vec<&O>> = contract_input
            .contract_infos
            .iter()
            .map(|x| {
                debug!(
                    "contract public keys: {}",
                    x.oracles
                        .public_keys
                        .iter()
                        .map(|k| k.to_string())
                        .collect::<Vec<String>>()
                        .join(", ")
                );
                x.oracles
                    .public_keys
                    .iter()
                    .map(|pubkey| match manager_oracles.get(pubkey) {
                        Some(x) => Ok(x),
                        None => Err(Error::InvalidParameters(
                            "Unknown oracle public key".to_string(),
                        )),
                    })
                    .collect::<Result<Vec<&O>, Error>>()
            })
            .collect::<Result<Vec<Vec<&O>>, Error>>()?;

        let mut oracle_announcements = Vec::new();

        for oracles in oracle_set {
            let mut announcements = Vec::new();
            for oracle in oracles {
                announcements.push(oracle.get_announcement(event_id).await?);
            }
            oracle_announcements.push(announcements)
        }

        let (offered_contract, offer_msg) = crate::dlc_manager::contract_updater::offer_contract(
            &self.secp,
            contract_input,
            oracle_announcements,
            refund_delay,
            &counter_party,
            &self.wallet,
            &self.blockchain,
            &self.time,
            calculate_denominator_from_basis_points(protocol_fee_basis_points),
            fee_address,
        )?;

        offered_contract.validate()?;

        self.store.create_contract(&offered_contract).await?;

        Ok(offer_msg)
    }

    /// Function to call to accept a DLC for which an offer was received.
    pub async fn accept_contract_offer(
        &self,
        contract_id: &ContractId,
    ) -> Result<(ContractId, PublicKey, AcceptDlc), Error> {
        let offered_contract =
            get_contract_in_state!(self, contract_id, Offered, None as Option<PublicKey>)?;

        let counter_party = offered_contract.counter_party;

        let (accepted_contract, accept_msg) = accept_contract(
            &self.secp,
            &offered_contract,
            &self.wallet,
            &self.blockchain,
        )?;

        self.wallet.import_address(&Address::p2wsh(
            &accepted_contract.dlc_transactions.funding_script_pubkey,
            self.blockchain.get_network_async().await?,
        ))?;

        let contract_id = accepted_contract.get_contract_id();

        self.store
            .update_contract(&Contract::Accepted(accepted_contract))
            .await?;

        Ok((contract_id, counter_party, accept_msg))
    }

    /// Function to call to check the state of the currently executing DLCs and
    /// update them if possible.
    pub async fn periodic_check(&self) -> Result<Vec<(ContractId, String)>, Error> {
        let mut affected_contracts = Vec::<(ContractId, String)>::new();
        affected_contracts.extend_from_slice(&self.check_signed_contracts().await?);
        affected_contracts.extend_from_slice(&self.check_confirmed_contracts().await?);
        affected_contracts.extend_from_slice(&self.check_preclosed_contracts().await?);

        Ok(affected_contracts)
    }

    async fn on_offer_message(
        &self,
        offered_message: &OfferDlc,
        counter_party: PublicKey,
    ) -> Result<(), Error> {
        offered_message.validate(&self.secp, 0, FIFTY_YEARS)?;
        let contract: OfferedContract =
            OfferedContract::try_from_offer_dlc(offered_message, counter_party)?;
        contract.validate()?;

        if self.store.get_contract(&contract.id).await?.is_some() {
            return Err(Error::InvalidParameters(
                "Contract with identical id already exists".to_string(),
            ));
        }

        self.store.create_contract(&contract).await?;

        Ok(())
    }

    async fn on_accept_message(
        &self,
        accept_msg: &AcceptDlc,
        counter_party: &PublicKey,
    ) -> Result<DlcMessage, Error> {
        let offered_contract = get_contract_in_state!(
            self,
            &accept_msg.temporary_contract_id,
            Offered,
            Some(*counter_party)
        )?;

        let (signed_contract, signed_msg) = match verify_accepted_and_sign_contract(
            &self.secp,
            &offered_contract,
            accept_msg,
            &self.wallet,
        ) {
            Ok(contract) => contract,
            Err(e) => {
                return self
                    .accept_fail_on_error(offered_contract, accept_msg.clone(), e)
                    .await
            }
        };

        self.wallet.import_address(&Address::p2wsh(
            &signed_contract
                .accepted_contract
                .dlc_transactions
                .funding_script_pubkey,
            self.blockchain.get_network_async().await?,
        ))?;

        self.store
            .update_contract(&Contract::Signed(signed_contract))
            .await?;

        Ok(DlcMessage::Sign(signed_msg))
    }

    async fn on_sign_message(
        &self,
        sign_message: &SignDlc,
        peer_id: &PublicKey,
    ) -> Result<(), Error> {
        let accepted_contract =
            get_contract_in_state!(self, &sign_message.contract_id, Accepted, Some(*peer_id))?;

        let (signed_contract, fund_tx) =
            match crate::dlc_manager::contract_updater::verify_signed_contract(
                &self.secp,
                &accepted_contract,
                sign_message,
                &self.wallet,
            ) {
                Ok(contract) => contract,
                Err(e) => {
                    return self
                        .sign_fail_on_error(accepted_contract, sign_message.clone(), e)
                        .await
                }
            };

        self.store
            .update_contract(&Contract::Signed(signed_contract))
            .await?;

        self.blockchain.send_transaction_async(&fund_tx).await?;

        Ok(())
    }

    async fn sign_fail_on_error<R>(
        &self,
        accepted_contract: AcceptedContract,
        sign_message: SignDlc,
        e: Error,
    ) -> Result<R, Error> {
        error!("Error in on_sign {}", e);
        self.store
            .update_contract(&Contract::FailedSign(FailedSignContract {
                accepted_contract,
                sign_message,
                error_message: e.to_string(),
            }))
            .await?;
        Err(e)
    }

    async fn accept_fail_on_error<R>(
        &self,
        offered_contract: OfferedContract,
        accept_message: AcceptDlc,
        e: Error,
    ) -> Result<R, Error> {
        error!("Error in on_accept {}", e);
        self.store
            .update_contract(&Contract::FailedAccept(FailedAcceptContract {
                offered_contract,
                accept_message,
                error_message: e.to_string(),
            }))
            .await?;
        Err(e)
    }

    async fn check_signed_contract(&self, contract: &SignedContract) -> Result<bool, Error> {
        let confirmations = self
            .blockchain
            .get_transaction_confirmations_async(
                &contract.accepted_contract.dlc_transactions.fund.txid(),
            )
            .await?;
        if confirmations >= NB_CONFIRMATIONS {
            self.store
                .update_contract(&Contract::Confirmed(contract.clone()))
                .await?;
            return Ok(true);
        }
        Ok(false)
    }

    async fn check_signed_contracts(&self) -> Result<Vec<(ContractId, String)>, Error> {
        let mut contracts_to_confirm = Vec::new();
        for c in self.store.get_signed_contracts().await? {
            match self.check_signed_contract(&c).await {
                Ok(true) => {
                    let contract_id = c.accepted_contract.get_contract_id();
                    let oracle_event_id = c
                        .accepted_contract
                        .offered_contract
                        .contract_info
                        .first()
                        .and_then(|info| info.oracle_announcements.first())
                        .map(|announcement| announcement.oracle_event.event_id.clone())
                        .ok_or(Error::InvalidState("Missing oracle event ID".to_string()))?;

                    contracts_to_confirm.push((contract_id, oracle_event_id));
                }
                Ok(false) => (),
                Err(e) => error!(
                    "Error checking signed contract {}: {}",
                    c.accepted_contract.get_contract_id_string(),
                    e
                ),
            }
        }

        Ok(contracts_to_confirm)
    }

    async fn check_confirmed_contracts(&self) -> Result<Vec<(ContractId, String)>, Error> {
        let mut contracts_to_close = Vec::new();
        for c in self.store.get_confirmed_contracts().await? {
            // Confirmed contracts from channel are processed in channel specific methods.
            if c.channel_id.is_some() {
                continue;
            }
            match self.check_confirmed_contract(&c).await {
                Err(e) => {
                    error!(
                        "Error checking confirmed contract {}: {}",
                        c.accepted_contract.get_contract_id_string(),
                        e
                    )
                }
                Ok(true) => {
                    let contract_id = c.accepted_contract.get_contract_id();
                    let oracle_event_id = c
                        .accepted_contract
                        .offered_contract
                        .contract_info
                        .first()
                        .and_then(|info| info.oracle_announcements.first())
                        .map(|announcement| announcement.oracle_event.event_id.clone())
                        .ok_or(Error::InvalidState("Missing oracle event ID".to_string()))?;

                    contracts_to_close.push((contract_id, oracle_event_id));
                }
                Ok(false) => (),
            }
        }

        Ok(contracts_to_close)
    }

    async fn get_closable_contract_info<'a>(
        &'a self,
        contract: &'a SignedContract,
    ) -> Result<ClosableContractInfo<'a>, Error> {
        let manager_oracles = match &self.oracles {
            // Oracles is now an optional field, so check here before continuing.
            Some(oracles) => oracles,
            None => {
                return Err(Error::InvalidParameters(
                    "Manager instantiated without oracles, get_closable_contract_info function not supported"
                        .to_string(),
                ));
            }
        };
        let contract_infos = &contract.accepted_contract.offered_contract.contract_info;
        let adaptor_infos = &contract.accepted_contract.adaptor_infos;
        for (contract_info, adaptor_info) in contract_infos.iter().zip(adaptor_infos.iter()) {
            let announcements: Vec<(usize, &OracleAnnouncement)> = contract_info
                .oracle_announcements
                .iter()
                .enumerate()
                .collect();

            if announcements.len() >= contract_info.threshold {
                let attestations: Vec<_> = futures::future::join_all(announcements.iter().map(
                    |(i, announcement)| async move {
                        let oracle = match manager_oracles.get(&announcement.oracle_public_key) {
                            Some(x) => x,
                            None => {
                                return Err(Error::InvalidParameters(
                                    "Unknown oracle public key".to_string(),
                                ));
                            }
                        };

                        let attestation = oracle
                            .get_attestation(&announcement.oracle_event.event_id)
                            .await
                            .map_err(|err| Error::OracleError(err.to_string()))?;

                        Ok((*i, attestation))
                    },
                ))
                .await;
                let attestations: Vec<_> = attestations
                    .iter()
                    .filter(|&pair| pair.is_ok())
                    .flat_map(|pair| {
                        pair.as_ref()
                            .map(|(i, attestation)| (*i, attestation.clone()))
                    })
                    .collect();

                if attestations.len() >= contract_info.threshold {
                    return Ok(Some((contract_info, adaptor_info, attestations)));
                }
            }
        }

        Ok(None)
    }

    async fn check_confirmed_contract(&self, contract: &SignedContract) -> Result<bool, Error> {
        let closable_contract_info = self.get_closable_contract_info(contract).await;
        if let Ok(Some((contract_info, adaptor_info, attestations))) = closable_contract_info {
            let cet = crate::dlc_manager::contract_updater::get_signed_cet(
                &self.secp,
                contract,
                contract_info,
                adaptor_info,
                &attestations,
                &self.wallet,
            )?;
            match self
                .close_contract(
                    contract,
                    cet,
                    attestations.iter().map(|x| x.1.clone()).collect(),
                )
                .await
            {
                Ok(closed_contract) => {
                    self.store.update_contract(&closed_contract).await?;
                    return Ok(true);
                }
                Err(e) => {
                    warn!(
                        "Failed to close contract {}: {}",
                        contract.accepted_contract.get_contract_id_string(),
                        e
                    );
                    return Err(e);
                }
            }
        }
        self.check_refund(contract).await?;

        Ok(false)
    }

    async fn check_preclosed_contracts(&self) -> Result<Vec<(ContractId, String)>, Error> {
        let mut contracts_to_close = Vec::new();
        for c in self.store.get_preclosed_contracts().await? {
            match self.check_preclosed_contract(&c).await {
                Ok(true) => {
                    let contract_id = c.signed_contract.accepted_contract.get_contract_id();
                    let oracle_event_id = c
                        .signed_contract
                        .accepted_contract
                        .offered_contract
                        .contract_info
                        .first()
                        .and_then(|info| info.oracle_announcements.first())
                        .map(|announcement| announcement.oracle_event.event_id.clone())
                        .ok_or(Error::InvalidState("Missing oracle event ID".to_string()))?;

                    contracts_to_close.push((contract_id, oracle_event_id));
                }
                Ok(false) => (),
                Err(e) => error!(
                    "Error checking pre-closed contract {}: {}",
                    c.signed_contract.accepted_contract.get_contract_id_string(),
                    e
                ),
            }
        }

        Ok(contracts_to_close)
    }

    async fn check_preclosed_contract(&self, contract: &PreClosedContract) -> Result<bool, Error> {
        let broadcasted_txid = contract.signed_cet.txid();
        let confirmations = self
            .blockchain
            .get_transaction_confirmations_async(&broadcasted_txid)
            .await?;
        if confirmations >= NB_CONFIRMATIONS {
            let closed_contract = ClosedContract {
                attestations: contract.attestations.clone(),
                signed_cet: Some(contract.signed_cet.clone()),
                contract_id: contract.signed_contract.accepted_contract.get_contract_id(),
                temporary_contract_id: contract
                    .signed_contract
                    .accepted_contract
                    .offered_contract
                    .id,
                counter_party_id: contract
                    .signed_contract
                    .accepted_contract
                    .offered_contract
                    .counter_party,
                pnl: contract
                    .signed_contract
                    .accepted_contract
                    .compute_pnl(&contract.signed_cet),
            };
            self.store
                .update_contract(&Contract::Closed(closed_contract.clone()))
                .await?;
            return Ok(true);
        }

        Ok(false)
    }

    async fn close_contract(
        &self,
        contract: &SignedContract,
        signed_cet: Transaction,
        attestations: Vec<OracleAttestation>,
    ) -> Result<Contract, Error> {
        let confirmations = self
            .blockchain
            .get_transaction_confirmations_async(&signed_cet.txid())
            .await?;

        // Put it here for post-close, and here's the btc txid too.
        // But perhaps we'd rather have it in the final close place, and
        // only set it after 6 confirmations

        if confirmations < 1 {
            // TODO(tibo): if this fails because another tx is already in
            // mempool or blockchain, we might have been cheated. There is
            // not much to be done apart from possibly extracting a fraud
            // proof but ideally it should be handled.
            self.blockchain.send_transaction_async(&signed_cet).await?;

            let preclosed_contract = PreClosedContract {
                signed_contract: contract.clone(),
                attestations: Some(attestations),
                signed_cet,
            };

            return Ok(Contract::PreClosed(preclosed_contract));
        } else if confirmations < NB_CONFIRMATIONS {
            let preclosed_contract = PreClosedContract {
                signed_contract: contract.clone(),
                attestations: Some(attestations),
                signed_cet,
            };

            return Ok(Contract::PreClosed(preclosed_contract));
        }

        let closed_contract = ClosedContract {
            attestations: Some(attestations.to_vec()),
            pnl: contract.accepted_contract.compute_pnl(&signed_cet),
            signed_cet: Some(signed_cet),
            contract_id: contract.accepted_contract.get_contract_id(),
            temporary_contract_id: contract.accepted_contract.offered_contract.id,
            counter_party_id: contract.accepted_contract.offered_contract.counter_party,
        };

        Ok(Contract::Closed(closed_contract))
    }

    async fn get_json(&self, path: &str) -> Result<serde_json::Value, Error> {
        reqwest::get(path)
            .await
            .map_err(|x| Error::WalletError(Box::new(x)))?
            .json::<serde_json::Value>()
            .await
            .map_err(|x| Error::WalletError(Box::new(x)))
    }

    async fn get_unixtime(&self) -> Result<u64, Error> {
        let path = "https://worldtimeapi.org/api/timezone/Etc/UTC";
        let v = match self.get_json(path).await {
            Ok(v) => v,
            Err(e) => {
                return Err(Error::WalletError(
                    format!("Error getting unixtime: {e}").into(),
                ))
            }
        };

        let unixtime = match v["unixtime"].as_u64() {
            //call to_string instead of as_str and watch your world crumble to pieces
            None => return Err(Error::WalletError("unable to get unixtime".into())),
            Some(s) => s,
        };

        Ok(unixtime)
    }

    async fn check_refund(&self, contract: &SignedContract) -> Result<(), Error> {
        // TODO(tibo): should check for confirmation of refund before updating state
        // use reqwest to fetch the current time

        let unixtime = self.get_unixtime().await?;
        if contract
            .accepted_contract
            .dlc_transactions
            .refund
            .lock_time
            .0 as u64
            <= unixtime
        {
            let accepted_contract = &contract.accepted_contract;
            let refund = accepted_contract.dlc_transactions.refund.clone();
            let confirmations = self
                .blockchain
                .get_transaction_confirmations_async(&refund.txid())
                .await?;
            if confirmations == 0 {
                let refund = crate::dlc_manager::contract_updater::get_signed_refund(
                    &self.secp,
                    contract,
                    &self.wallet,
                )?;
                self.blockchain.send_transaction_async(&refund).await?;
            }

            self.store
                .update_contract(&Contract::Refunded(contract.clone()))
                .await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn test_calculate_denominator_from_basis_points() {
        let basis_points = 100;
        let denominator = super::calculate_denominator_from_basis_points(basis_points);
        assert_eq!(denominator, 100);

        let basis_points = 10;
        let denominator = super::calculate_denominator_from_basis_points(basis_points);
        assert_eq!(denominator, 1000);

        let basis_points = 50;
        let denominator = super::calculate_denominator_from_basis_points(basis_points);
        assert_eq!(denominator, 200);

        let basis_points = 200;
        let denominator = super::calculate_denominator_from_basis_points(basis_points);
        assert_eq!(denominator, 50);

        let basis_points = 1000;
        let denominator = super::calculate_denominator_from_basis_points(basis_points);
        assert_eq!(denominator, 10);

        let basis_points = 0;
        let denominator = super::calculate_denominator_from_basis_points(basis_points);
        assert_eq!(denominator, 0);
    }
}

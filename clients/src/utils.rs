use dlc_manager::contract::offered_contract::OfferedContract;
use dlc_manager::contract::signed_contract::SignedContract;
use dlc_manager::contract::{Contract, PreClosedContract};
use dlc_manager::error::Error;

use dlc_manager::contract::accepted_contract::AcceptedContract;
use dlc_manager::contract::ser::Serializable;
use dlc_manager::contract::{ClosedContract, FailedAcceptContract, FailedSignContract};
use std::fmt::Write as _;

pub fn to_storage_error<T>(e: T) -> Error
where
    T: std::fmt::Display,
{
    Error::StorageError(e.to_string())
}

macro_rules! convertible_enum {
    (enum $name:ident {
        $($vname:ident $(= $val:expr)?,)*;
        $($tname:ident $(= $tval:expr)?,)*
    }, $input:ident) => {
        #[derive(Debug)]
        enum $name {
            $($vname $(= $val)?,)*
            $($tname $(= $tval)?,)*
        }

        impl From<$name> for u8 {
            fn from(prefix: $name) -> u8 {
                prefix as u8
            }
        }

        impl std::convert::TryFrom<u8> for $name {
            type Error = Error;

            fn try_from(v: u8) -> Result<Self, Self::Error> {
                match v {
                    $(x if x == u8::from($name::$vname) => Ok($name::$vname),)*
                    $(x if x == u8::from($name::$tname) => Ok($name::$tname),)*
                    _ => Err(Error::StorageError("Unknown prefix".to_string())),
                }
            }
        }

        impl $name {
            fn get_prefix(input: &$input) -> u8 {
                let prefix = match input {
                    $($input::$vname(_) => $name::$vname,)*
                    $($input::$tname{..} => $name::$tname,)*
                };
                prefix.into()
            }
        }
    }
}

convertible_enum!(
    enum ContractPrefix {
        Offered = 1,
        Accepted,
        Signed,
        Confirmed,
        PreClosed,
        Closed,
        FailedAccept,
        FailedSign,
        Refunded,
        Rejected,;
    },
    Contract
);

pub fn serialize_contract(contract: &Contract) -> Result<Vec<u8>, ::std::io::Error> {
    let serialized = match contract {
        Contract::Offered(c) => c.serialize(),
        Contract::Accepted(c) => c.serialize(),
        Contract::Signed(c) | Contract::Confirmed(c) | Contract::Refunded(c) => c.serialize(),
        Contract::FailedAccept(c) => c.serialize(),
        Contract::FailedSign(c) => c.serialize(),
        Contract::PreClosed(c) => c.serialize(),
        Contract::Closed(c) => c.serialize(),
        Contract::Rejected(c) => c.serialize(),
    };
    let mut serialized = serialized?;
    let mut res = Vec::with_capacity(serialized.len() + 1);
    res.push(ContractPrefix::get_prefix(contract));
    res.append(&mut serialized);
    Ok(res)
}

pub fn deserialize_contract(buff: &Vec<u8>) -> Result<Contract, Error> {
    let mut cursor = ::std::io::Cursor::new(buff);
    let mut prefix = [0u8; 1];
    std::io::Read::read_exact(&mut cursor, &mut prefix)?;
    let contract_prefix: ContractPrefix = prefix[0].try_into()?;
    let contract = match contract_prefix {
        ContractPrefix::Offered => {
            Contract::Offered(OfferedContract::deserialize(&mut cursor).map_err(to_storage_error)?)
        }
        ContractPrefix::Accepted => Contract::Accepted(
            AcceptedContract::deserialize(&mut cursor).map_err(to_storage_error)?,
        ),
        ContractPrefix::Signed => {
            Contract::Signed(SignedContract::deserialize(&mut cursor).map_err(to_storage_error)?)
        }
        ContractPrefix::Confirmed => {
            Contract::Confirmed(SignedContract::deserialize(&mut cursor).map_err(to_storage_error)?)
        }
        ContractPrefix::PreClosed => Contract::PreClosed(
            PreClosedContract::deserialize(&mut cursor).map_err(to_storage_error)?,
        ),
        ContractPrefix::Closed => {
            Contract::Closed(ClosedContract::deserialize(&mut cursor).map_err(to_storage_error)?)
        }
        ContractPrefix::FailedAccept => Contract::FailedAccept(
            FailedAcceptContract::deserialize(&mut cursor).map_err(to_storage_error)?,
        ),
        ContractPrefix::FailedSign => Contract::FailedSign(
            FailedSignContract::deserialize(&mut cursor).map_err(to_storage_error)?,
        ),
        ContractPrefix::Refunded => {
            Contract::Refunded(SignedContract::deserialize(&mut cursor).map_err(to_storage_error)?)
        }
        ContractPrefix::Rejected => {
            Contract::Rejected(OfferedContract::deserialize(&mut cursor).map_err(to_storage_error)?)
        }
    };
    Ok(contract)
}

pub fn get_contract_state_str(contract: &Contract) -> String {
    let state = match contract {
        Contract::Offered(_) => "offered",
        Contract::Accepted(_) => "accepted",
        Contract::Signed(_) => "signed",
        Contract::Confirmed(_) => "confirmed",
        Contract::PreClosed(_) => "pre_closed",
        Contract::Closed(_) => "closed",
        Contract::Refunded(_) => "refunded",
        Contract::FailedAccept(_) => "failed_accept",
        Contract::FailedSign(_) => "failed_sign",
        Contract::Rejected(_) => "rejected",
    };
    state.to_string()
}

pub fn get_contract_id_string(contract_id: [u8; 32]) -> String {
    let mut string_id = String::with_capacity(32 * 2 + 2);
    string_id.push_str("0x");
    for i in &contract_id {
        write!(string_id, "{:02x}", i).unwrap();
    }
    string_id
}

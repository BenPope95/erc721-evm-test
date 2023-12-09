#[path = "./abi/erc721.rs"]
mod erc721;

mod helpers;
mod pb;
mod abi;

use pb::schema::{Approval, Approvals, Transfer, Transfers, Contracts, Contract};
use substreams::pb::substreams::Clock;
use substreams_entity_change::{pb::entity::EntityChanges, tables::Tables};
use substreams_ethereum::{pb::eth, Event};
use primitive_types::H256;
use std::collections::HashMap;

use helpers::*;

use erc721::events::{Approval as ApprovalEvent, Transfer as TransferEvent};

pub const ADDRESS: &str = "0xBC4CA0EdA7647A8aB7C2061c2E118A18a936f13D";
const START_BLOCK: u64 = 12287507;

#[substreams::handlers::map]
pub fn map_blocks(params: String, blk: eth::Block) -> Result<Contracts, Error> {
    log::info!("map_blocks: {}", params);
    let contracts = blk
        .transaction_traces
        .into_iter()
        .filter(|tx| tx.status == 1)
        .flat_map(|tx| {
            tx.calls
                .into_iter()
                .filter(|call| !call.state_reverted)
                .filter(|call| call.call_type == eth::CallType::Create as i32)
                .map(|call| ContractCreation {
                    address: call.address,
                    // Better deal with code_changes being a list, last is good for now
                    code: call.code_changes.into_iter().last().unwrap().new_code,
                    storage_changes: call
                        .storage_changes
                        .into_iter()
                        .map(|s| (H256::from_slice(s.key.as_ref()), s.new_value))
                        .collect(),
                })
        })
        .collect::<Vec<_>>();

    Ok(Contracts {
        contracts: contracts.into_iter().filter_map(process_contract).collect(),
    })
}

#[substreams::handlers::map]
fn map_transfers(block: eth::v2::Block) -> Result<Transfers, substreams::errors::Error> {
    let transfers = block
        .logs()
        .filter_map(|log| {
            TransferEvent::match_and_decode(log).map(|transfer| (transfer, format_hex(&log.receipt.transaction.hash)))
        })
        .map(|(transfer, hash)| Transfer {
            from: format_hex(&transfer.from),
            to: format_hex(&transfer.to),
            token_id: transfer.token_id.to_string(),
            tx_hash: hash,
            token_name: "".to_string(),
            token_symbol: "".to_string(),
            token_uri: "".to_string(),
        })
        .collect::<Vec<Transfer>>();

    Ok(Transfers { transfers })
}

#[substreams::handlers::map]
fn map_approvals(block: eth::v2::Block) -> Result<Approvals, substreams::errors::Error> {
    let approvals = block
        .logs()
        .filter_map(|log| {
            if format_hex(log.address()) == ADDRESS.to_lowercase() {
                if let Some(approval) = ApprovalEvent::match_and_decode(log) {
                    Some((approval, format_hex(&log.receipt.transaction.hash)))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .map(|(approval, hash)| Approval {
            owner: format_hex(&approval.owner),
            approved: format_hex(&approval.approved),
            token_id: approval.token_id.to_string(),
            tx_hash: hash,
            token_name: "".to_string(),
            token_symbol: "".to_string(),
            token_uri: "".to_string(),
        })
        .collect::<Vec<Approval>>();

    Ok(Approvals { approvals })
}

#[substreams::handlers::map]
pub fn graph_out(
    clock: Clock,
    transfers: Transfers,
    approvals: Approvals,
) -> Result<EntityChanges, substreams::errors::Error> {
    let mut tables = Tables::new();

    if clock.number == START_BLOCK {
        // Create the collection, we only need to do this once
        tables.create_row("Collection", ADDRESS.to_string());
    }

    transfers_to_table_changes(&mut tables, &transfers);

    approvals_to_table_changes(&mut tables, &approvals);

    Ok(tables.to_entity_changes())
}

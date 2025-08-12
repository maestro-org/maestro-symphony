use crate::serve::AppState;
use crate::serve::error::ServeError;
use crate::serve::types::ServeResponse;
use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};
use bitcoin::{Transaction, consensus::Decodable};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use tokio::sync::mpsc;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
pub struct SubmitTransactionRequest {
    /// Raw transaction bytes encoded as hex string
    pub raw_tx: String,
}

#[derive(Serialize, ToSchema)]
pub struct SubmitTransactionResponse {
    /// Transaction ID of the submitted transaction
    pub txid: String,
    /// Status of the submission
    pub status: String,
}

// Channel for sending transactions to the P2P layer
static TX_SUBMISSION_CHANNEL: OnceLock<mpsc::UnboundedSender<Transaction>> = OnceLock::new();

pub fn set_tx_submission_sender(sender: mpsc::UnboundedSender<Transaction>) {
    TX_SUBMISSION_CHANNEL
        .set(sender)
        .expect("Transaction submission channel already set");
}

pub fn get_tx_submission_sender() -> Option<&'static mpsc::UnboundedSender<Transaction>> {
    TX_SUBMISSION_CHANNEL.get()
}

#[utoipa::path(
    tag = "Transactions",
    post,
    path = "/transactions/submit",
    request_body(
        content = SubmitTransactionRequest,
        description = "Transaction to submit",
        content_type = "application/json"
    ),
    responses(
        (
            status = 200,
            description = "Transaction submitted successfully",
            body = ServeResponse<SubmitTransactionResponse>,
            example = json!({
                "data": {
                    "txid": "c0345bb5906257a05cdc2d11b6580ce75fdfe8b7ac09b7b2711d435e2ba0a9b3",
                    "status": "submitted"
                },
                "indexer_info": {
                    "chain_tip": {
                        "block_hash": "00000000000000108a4cd9755381003a01bea7998ca2d770fe09b576753ac7ef",
                        "block_height": 31633
                    },
                    "mempool_timestamp": null,
                    "estimated_blocks": []
                }
            })
        ),
        (status = 400, description = "Invalid transaction data"),
        (status = 500, description = "Internal server error"),
    )
)]
/// Propagate Transaction
///
/// Propagate a transaction to the Bitcoin network. If the transaction is not valid it will fail
/// silently as it is directly passed to a peer via P2P.
pub async fn submit_transaction(
    State(state): State<AppState>,
    Json(request): Json<SubmitTransactionRequest>,
) -> Result<impl IntoResponse, ServeError> {
    // Parse the raw transaction hex
    let tx_bytes = hex::decode(&request.raw_tx)
        .map_err(|_| ServeError::malformed_request("invalid hex encoding"))?;

    // Decode the transaction
    let transaction = Transaction::consensus_decode_from_finite_reader(&mut &tx_bytes[..])
        .map_err(|_| ServeError::malformed_request("invalid transaction data"))?;

    let txid = transaction.compute_txid();

    // Get the transaction submission channel
    let sender = get_tx_submission_sender()
        .ok_or_else(|| ServeError::internal("transaction submission not available"))?;

    // Send the transaction to the P2P layer for broadcasting
    sender
        .send(transaction)
        .map_err(|_| ServeError::internal("failed to queue transaction for submission"))?;

    // Get indexer info for response
    let (_, indexer_info) = state.start_reader(false).await?;

    let response_data = SubmitTransactionResponse {
        txid: txid.to_string(),
        status: "submitted".to_string(),
    };

    let response = ServeResponse {
        data: response_data,
        indexer_info,
    };

    Ok((StatusCode::OK, Json(response)))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/submit", post(submit_transaction))
}

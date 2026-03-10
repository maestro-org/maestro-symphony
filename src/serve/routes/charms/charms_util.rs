use crate::serve::error::ServeError;
use crate::serve::types::CharmAndValue;
use crate::storage::encdec::Decode;
use crate::sync::stages::index::indexers::custom::charms::tables::UtxoCharms;

/// Decode UtxoCharms (raw bytes from UTXO extended data) into a list of CharmAndValue for API responses.
pub fn decode_utxo_charms(raw: &[u8]) -> Result<Vec<CharmAndValue>, ServeError> {
    let utxo_charms = UtxoCharms::decode_all(raw)?;

    utxo_charms
        .into_iter()
        .map(|(app_bytes, cbor_bytes)| {
            let app = String::from_utf8(app_bytes)
                .map_err(|_| ServeError::internal("invalid charm app encoding"))?;
            let data = charms_data::Data::try_from_bytes(&cbor_bytes)
                .map_err(|e| ServeError::internal(format!("failed to decode charm data: {e}")))?;
            let value = serde_json::to_value(&data).map_err(|e| {
                ServeError::internal(format!("failed to serialize charm value: {e}"))
            })?;
            Ok(CharmAndValue { app, value })
        })
        .collect()
}

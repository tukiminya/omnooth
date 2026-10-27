use omnooth_core::{parse_item_import_deep_link, ItemImportReceipt};
use std::collections::VecDeque;

/// Only non-secret receipt metadata is retained until the import worker is connected.
#[derive(Default)]
pub struct Receiver {
    pub received_count: u64,
    pub last_receipt: Option<ItemImportReceipt>,
    pub last_error: Option<String>,
    recent_sources: VecDeque<String>,
}

impl Receiver {
    pub fn receive(&mut self, url: &str) -> bool {
        match parse_item_import_deep_link(url) {
            Ok(parsed) => {
                let receipt = parsed.receipt().clone();
                // Startup delivery and live notifications can refer to the same request.
                if self.recent_sources.contains(&receipt.source_identity) {
                    return false;
                }
                self.recent_sources
                    .push_back(receipt.source_identity.clone());
                if self.recent_sources.len() > 32 {
                    self.recent_sources.pop_front();
                }
                self.received_count = self.received_count.saturating_add(1);
                self.last_receipt = Some(receipt);
                self.last_error = None;
            }
            Err(error) => self.last_error = Some(error.code().to_owned()),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::Receiver;

    #[test]
    fn startup_and_runtime_share_validation_and_deduplication() {
        let url = "booth-library-manager://item-import?item_id=42&downloadable_filename=Asset.zip&dlurl=https%3A%2F%2Fs6.booth.pm%2Ffiles%2Fasset.zip%3Fsignature%3Dsecret";
        let mut receiver = Receiver::default();
        assert!(receiver.receive(url));
        assert!(!receiver.receive(url));
        assert_eq!(receiver.received_count, 1);
        let receipt = serde_json::to_string(&receiver.last_receipt).unwrap();
        assert!(!receipt.contains("secret"));
        assert!(!receipt.contains("https"));
        assert!(receiver.receive("booth-library-manager://unsupported"));
        assert_eq!(receiver.received_count, 1);
        assert_eq!(receiver.last_error.as_deref(), Some("INVALID_DEEP_LINK"));
    }
}

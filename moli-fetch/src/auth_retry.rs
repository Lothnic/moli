/// Authentication restart policy for one logical request.
///
/// Retain this value across transport calls and authentication pauses. The
/// initial request is free; like Chromium's HttpNetworkTransaction, the 32nd
/// restart is rejected before sending another request.
#[derive(Clone, Debug, Default)]
pub struct AuthRetryBudget {
    restarts: u8,
}

impl AuthRetryBudget {
    pub const MAX_RESTARTS: u8 = 32;
    pub const ERROR_TEXT: &'static str = "net::ERR_TOO_MANY_RETRIES";

    pub fn try_restart(&mut self) -> bool {
        self.restarts = self.restarts.saturating_add(1);
        self.restarts < Self::MAX_RESTARTS
    }
}

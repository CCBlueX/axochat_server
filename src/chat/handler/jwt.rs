use crate::chat::{ChatServer, InternalId};
use crate::error::ClientError;

impl ChatServer {
    /// JWT login was replaced by LiquidBounce Accounts; the packets remain for old clients.
    pub(super) fn handle_jwt(&mut self, user_id: InternalId) {
        self.send_error(user_id, ClientError::NotSupported);
    }
}

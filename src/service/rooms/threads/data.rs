use crate::{PduEvent, Result};
use ruma::{api::client::threads::get_threads::v1::IncludeThreads, OwnedUserId, RoomId, UserId};

pub trait Data: Send + Sync {
    fn threads_until<'a>(
        &'a self,
        user_id: &'a UserId,
        room_id: &'a RoomId,
        until: u64,
        include: &'a IncludeThreads,
    ) -> Result<Box<dyn Iterator<Item = Result<(u64, PduEvent)>> + 'a>>;

    fn update_participants(&self, root_id: &[u8], participants: &[OwnedUserId]) -> Result<()>;
    fn get_participants(&self, root_id: &[u8]) -> Result<Option<Vec<OwnedUserId>>>;
}

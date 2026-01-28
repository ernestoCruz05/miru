use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use tracing::{debug, error};

pub struct DiscordRpc {
    client: Option<DiscordIpcClient>,
}

impl DiscordRpc {
    pub fn new(app_id: &str) -> Self {
        debug!("Initializing Discord RPC with ID: {}", app_id);
        let mut client = Some(DiscordIpcClient::new(app_id));

        
        if let Some(c) = &mut client {
             if let Err(e) = c.connect() {
                 debug!("Failed to connect to Discord RPC (is Discord running?): {}", e);
                 return Self { client: None };
             }
        }
        
        Self { client }
    }

    pub fn set_activity(&mut self, state: &str, details: &str) {
         if let Some(client) = &mut self.client {
             let payload = activity::Activity::new()
                 .state(state)
                 .details(details)
                 .assets(activity::Assets::new().large_image("logo").large_text("Miru")); 
             
             if let Err(e) = client.set_activity(payload) {
                 error!("Failed to set activity: {}", e);
                 // If broken pipe, maybe invalidate client? For now just log.
             }
         }
    }

    pub fn clear(&mut self) {
        if let Some(client) = &mut self.client {
            if let Err(e) = client.clear_activity() {
                debug!("Failed to clear activity: {}", e);
            }
        }
    }
}

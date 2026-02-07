//! Desktop notifications for miru
//!
//! Provides cross-platform notifications for:
//! - New episodes found for tracked series
//! - Completed downloads

use notify_rust::Notification;
use tracing::{debug, warn};

const APP_NAME: &str = "Miru";

pub struct Notifier {
    enabled: bool,
}

impl Notifier {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    pub fn new_episode(&self, series_title: &str, episode: u32) {
        if !self.enabled {
            return;
        }

        let body = format!("Episode {} is now available", episode);
        self.send(series_title, &body);
    }

    pub fn download_complete(&self, name: &str) {
        if !self.enabled {
            return;
        }

        self.send("Download Complete", name);
    }

    fn send(&self, summary: &str, body: &str) {
        debug!(summary = %summary, body = %body, "Sending notification");

        let result = Notification::new()
            .appname(APP_NAME)
            .summary(summary)
            .body(body)
            .timeout(5000)
            .show();

        if let Err(e) = result {
            warn!("Failed to send notification: {}", e);
        }
    }
}

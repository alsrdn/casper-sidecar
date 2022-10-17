use serde::{Deserialize, Serialize};

/// Default binding address for the SSE HTTP server.
///
/// Uses a fixed port per node, but binds on any interface.
const DEFAULT_ADDRESS: &str = "0.0.0.0:0";

/// Default number of SSEs to buffer.
const DEFAULT_EVENT_STREAM_BUFFER_LENGTH: u32 = 5000;

/// Default maximum number of subscribers.
const DEFAULT_MAX_CONCURRENT_SUBSCRIBERS: u32 = 100;

/// SSE HTTP server configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Address to bind event stream SSE HTTP server to.
    pub address: String,

    /// Number of SSEs to buffer.
    pub event_stream_buffer_length: u32,

    /// Default maximum number of subscribers across all event streams permitted at any one time.
    pub max_concurrent_subscribers: u32,
}

impl Config {
    /// Creates a default instance for `EventStreamServer`.
    pub fn new(
        address: Option<String>,
        buffer_length: Option<u32>,
        max_subscribers: Option<u32>,
    ) -> Self {
        Config {
            address: address.unwrap_or_else(|| DEFAULT_ADDRESS.to_string()),
            event_stream_buffer_length: buffer_length.unwrap_or(DEFAULT_EVENT_STREAM_BUFFER_LENGTH),
            max_concurrent_subscribers: max_subscribers
                .unwrap_or(DEFAULT_MAX_CONCURRENT_SUBSCRIBERS),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::new(None, None, None)
    }
}

mod actions;
mod client;
mod oauth;
mod rpc_events;

use actions::*;
use client::schedule_reconnect;

use std::sync::OnceLock;

use openaction::{
	OpenActionResult, async_trait, get_global_settings, global_events, register_action, run,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

// Built-in Discord application credentials for zero-configuration setup.
// Set at build time: DISCORD_CLIENT_ID=... DISCORD_CLIENT_SECRET=... cargo build
// When set, users only need to approve the OAuth dialog in Discord — no manual setup.
const DEFAULT_CLIENT_ID: &str = match option_env!("DISCORD_CLIENT_ID") {
	Some(v) => v,
	None => "",
};

const DEFAULT_CLIENT_SECRET: &str = match option_env!("DISCORD_CLIENT_SECRET") {
	Some(v) => v,
	None => "",
};

// Represents the persisted Discord configuration the Stream Deck host sends us.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct DiscordSettings {
	#[serde(rename = "clientId")]
	pub client_id: String,
	#[serde(rename = "clientSecret")]
	pub client_secret: String,
	#[serde(rename = "accessToken")]
	pub access_token: String,
	pub error: Option<String>,
}

impl DiscordSettings {
	/// Returns the effective client ID (user override or built-in default).
	pub fn effective_client_id(&self) -> &str {
		if self.client_id.is_empty() {
			DEFAULT_CLIENT_ID
		} else {
			&self.client_id
		}
	}

	/// Returns the effective client secret (user override or built-in default).
	pub fn effective_client_secret(&self) -> &str {
		if self.client_secret.is_empty() {
			DEFAULT_CLIENT_SECRET
		} else {
			&self.client_secret
		}
	}

	/// Whether usable credentials are available (either user-provided or built-in).
	pub fn has_credentials(&self) -> bool {
		!self.effective_client_id().is_empty() && !self.effective_client_secret().is_empty()
	}
}

// Global storage for the last-applied settings so every module can read/write them.
pub fn current_settings() -> &'static RwLock<DiscordSettings> {
	static SETTINGS: OnceLock<RwLock<DiscordSettings>> = OnceLock::new();
	SETTINGS.get_or_init(|| RwLock::new(DiscordSettings::default()))
}

// Handles global setting updates pushed from the Stream Deck host.
pub struct GlobalEventHandler;
#[async_trait]
impl global_events::GlobalEventHandler for GlobalEventHandler {
	async fn plugin_ready(&self) -> OpenActionResult<()> {
		get_global_settings().await
	}

	async fn did_receive_global_settings(
		&self,
		event: global_events::DidReceiveGlobalSettingsEvent,
	) -> OpenActionResult<()> {
		let settings: DiscordSettings =
			serde_json::from_value(event.payload.settings).unwrap_or_default();

		// Only react when the stored settings actually changed so we can avoid reconnect churn.
		let current = current_settings().read().await;
		let settings_changed = current.client_id != settings.client_id
			|| current.client_secret != settings.client_secret
			|| current.access_token != settings.access_token
			|| current.access_token.is_empty();
		drop(current);

		if settings_changed {
			log::info!("Global settings changed, reinitializing Discord client");

			// Persist the new configuration before attempting to reconnect.
			*current_settings().write().await = settings;

			schedule_reconnect();
		}

		Ok(())
	}
}

#[tokio::main]
async fn main() -> OpenActionResult<()> {
	{
		use simplelog::*;
		if let Err(error) = TermLogger::init(
			LevelFilter::Debug,
			Config::default(),
			TerminalMode::Stdout,
			ColorChoice::Never,
		) {
			eprintln!("Logger initialization failed: {}", error);
		}
	}

	global_events::set_global_event_handler(&GlobalEventHandler);
	register_action(ToggleMuteAction).await;
	register_action(ToggleDeafenAction).await;
	register_action(PushToMuteAction).await;
	register_action(PushToTalkAction).await;
	register_action(TogglePushToTalkAction).await;

	run(std::env::args().collect()).await
}

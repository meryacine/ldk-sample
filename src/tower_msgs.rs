//! Trying out custom messages.
//! Using some messages found in [BOLT #13]: https://github.com/sr-gi/bolt13/blob/master/13-watchtowers.md

use bitcoin::secp256k1::key::PublicKey;
use core::mem;
use lightning::ln::channelmanager::SimpleArcChannelManager;
use lightning::ln::msgs::{DecodeError, ErrorAction, LightningError, WarningMessage};
use lightning::ln::peer_handler::{CustomMessageHandler, PeerManager};
use lightning::ln::wire::{CustomMessageReader, Type};
use lightning::routing::network_graph::{NetGraphMsgHandler, NetworkGraph};
use lightning::util::logger;
use lightning::util::ser::{Readable, Writeable, Writer};
use std::io;
use std::sync::{Arc, Mutex};

/// The register message: The user would send this message to the tower to
/// register for the watching service.
#[derive(Debug)]
pub struct Register {
	pub pubkey: PublicKey,
	pub appointment_slots: u32,
	pub subscription_period: u32,
}

/// The subscription details message: The tower would reply to a user's register
/// message with this message, specifying the maximum appointment size and the
/// subscription fee in msat.
#[derive(Debug)]
pub struct SubscriptionDetails {
	pub appointment_max_size: u16,
	pub amount_msat: u32,
}

/// Defines a constant type identifier for reading messages from the wire.
/// Just like the private [`lightning::ln::wire::Encode`].
pub trait Encode {
	/// The type identifying the message payload.
	const TYPE: u16;
}

impl Encode for Register {
	// An arbitrary even type.
	const TYPE: u16 = 45768;
}

/// Make the register message readable.
impl Readable for Register {
	fn read<R: io::Read>(reader: &mut R) -> Result<Self, DecodeError> {
		Ok(Self {
			pubkey: Readable::read(reader)?,
			appointment_slots: Readable::read(reader)?,
			subscription_period: Readable::read(reader)?,
		})
	}
}

/// The tower won't actually need this implementation, only the registerer will.
impl Writeable for Register {
	fn write<W: Writer>(&self, writer: &mut W) -> Result<(), io::Error> {
		self.pubkey.write(writer)?;
		self.appointment_slots.write(writer)?;
		self.subscription_period.write(writer)?;
		Ok(())
	}
}

impl Encode for SubscriptionDetails {
	// An arbitrary even type.
	const TYPE: u16 = 45770;
}

/// The tower won't actually need this implementation, only the registerer will.
impl Readable for SubscriptionDetails {
	fn read<R: io::Read>(reader: &mut R) -> Result<Self, DecodeError> {
		Ok(Self {
			appointment_max_size: Readable::read(reader)?,
			amount_msat: Readable::read(reader)?,
		})
	}
}

/// Make the subscription details message writable.
impl Writeable for SubscriptionDetails {
	fn write<W: Writer>(&self, writer: &mut W) -> Result<(), io::Error> {
		self.appointment_max_size.write(writer)?;
		self.amount_msat.write(writer)?;
		Ok(())
	}
}

/// A listing of all the messages the tower can send and receive.
#[derive(Debug)]
pub enum TowerMessage {
	Register(Register),
	SubscriptionDetails(SubscriptionDetails),
	// Other msgs go here ...
}

impl Type for TowerMessage {
	fn type_id(&self) -> u16 {
		match self {
			TowerMessage::Register(..) => Register::TYPE,
			TowerMessage::SubscriptionDetails(..) => SubscriptionDetails::TYPE,
		}
	}
}

impl Writeable for TowerMessage {
	fn write<W: Writer>(&self, writer: &mut W) -> Result<(), io::Error> {
		match self {
			// A tower won't normally send register messages to anybody so we
			// don't need to implement this case and we can return an error instead.
			TowerMessage::Register(msg) => msg.write(writer)?,
			TowerMessage::SubscriptionDetails(msg) => {
				msg.write(writer)?;
			}
		}
		Ok(())
	}
}

/// A handler to handle the incoming [`TowerMessage`]s.
pub struct TowerMessageHandler {
	msg_q: Mutex<Vec<(PublicKey, TowerMessage)>>,
}

impl TowerMessageHandler {
	pub fn new() -> Self {
		Self { msg_q: Mutex::new(Vec::new()) }
	}

	pub fn handle_tower_message(msg: TowerMessage) -> Result<TowerMessage, LightningError> {
		match msg {
			TowerMessage::Register(msg) => {
				println!(
					"Received a Register message: {:?}.\nResponding with a SubscriptionDetails message.", msg
				);
				let appointment_max_size = 30;
				// Pay for the Storage * Time.
				let amount_msat = msg.appointment_slots * msg.subscription_period;
				Ok(TowerMessage::SubscriptionDetails(SubscriptionDetails {
					appointment_max_size,
					amount_msat,
				}))
			}
			TowerMessage::SubscriptionDetails(msg) => {
				println!("Received a SubscriptionDetails message: {:?}.\nIgnoring it.", msg);
				// A tower shouldn't normally receive this message.
				Err(LightningError {
					err: "A SubscriptionDetails message wasn't expected!".to_string(),
					action: ErrorAction::SendWarningMessage {
						msg: WarningMessage {
							channel_id: [0; 32],
							data:
								"You sent me a SubscriptionDetails message but I didn't register!"
									.to_string(),
						},
						log_level: logger::Level::Debug,
					},
				})
			}
		}
	}

	pub fn send_message(&self, pubkey: &PublicKey, msg: TowerMessage) {
		self.msg_q.lock().unwrap().push((pubkey.clone(), msg))
	}
}

impl CustomMessageReader for TowerMessageHandler {
	type CustomMessage = TowerMessage;

	fn read<R: io::Read>(
		&self, message_type: u16, buffer: &mut R,
	) -> Result<Option<TowerMessage>, DecodeError> {
		match message_type {
			Register::TYPE => Ok(Some(TowerMessage::Register(Readable::read(buffer)?))),
			// Similar to the writable register message, we won't ever need to read
			// a subscription details message. We could have return an error here instead.
			SubscriptionDetails::TYPE => {
				Ok(Some(TowerMessage::SubscriptionDetails(Readable::read(buffer)?)))
			}
			// Unknown message.
			_ => Ok(None),
		}
	}
}

impl CustomMessageHandler for TowerMessageHandler {
	fn handle_custom_message(
		&self, msg: TowerMessage, sender_node_id: &PublicKey,
	) -> Result<(), LightningError> {
		Ok(self
			.msg_q
			.lock()
			.unwrap()
			.push((sender_node_id.clone(), Self::handle_tower_message(msg)?)))
	}

	fn get_and_clear_pending_msg(&self) -> Vec<(PublicKey, TowerMessage)> {
		mem::replace(&mut self.msg_q.lock().unwrap(), Vec::new())
	}
}

/// A type similar to [`SimpleArcPeerManager`] but uses [`TowerMessageHandler`]
/// instead of [`IgnoringMessageHandler`] for the handling of custom messages.
///
/// [`SimpleArcPeerManager`]: lightning::ln::peer_handler::SimpleArcPeerManager
/// [`IgnoringMessageHandler`]: lightning::ln::peer_handler::IgnoringMessageHandler
pub type SimpleTowerArcPeerManager<SD, M, T, F, C, L> = PeerManager<
	SD,
	Arc<SimpleArcChannelManager<M, T, F, L>>,
	Arc<NetGraphMsgHandler<Arc<NetworkGraph>, Arc<C>, Arc<L>>>,
	Arc<L>,
	Arc<TowerMessageHandler>,
>;

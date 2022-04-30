# LDK Custom Messages Demo

LDK supports sending and receiveing custom messages. This repo exploits this feature.
This is a patched ldk-sample that is capable of sending and receiving/handling a custom message named [`Register`](https://github.com/sr-gi/bolt13/blob/1d50640565407621f2436de71bc8ae7d79d5e470/13-watchtowers.md#the-register_top_up-message) found in bolt13 (watchtowers bold). The response to the `Register` message is the [`SubscriptionDetails`](https://github.com/sr-gi/bolt13/blob/1d50640565407621f2436de71bc8ae7d79d5e470/13-watchtowers.md#the-subscription_details-message) message.

Having custom messages support is useful in this case for implementing non-standardized lightning messages inside lightning applications.

This is a walk through on how to define and integrate custom messages and their handler into your LDK application.

TL;DR: Basically what you'll want to do is define some enum listing your custom messages. Then you'll implement [`lightning::ln::wire::Type`](https://github.com/lightningdevkit/rust-lightning/blob/9bdce47f0e0516e37c89c09f1975dfc06b5870b1/lightning/src/ln/wire.rs#L258) for that enum (which will require a method to return the message id and that you implement [`lightning::util::ser::Writeable`](https://github.com/lightningdevkit/rust-lightning/blob/9bdce47f0e0516e37c89c09f1975dfc06b5870b1/lightning/src/util/ser.rs#L165) to serialize it). Then you'll implement [`lightning::ln::wire::CustomMessageReader`](https://github.com/lightningdevkit/rust-lightning/blob/9bdce47f0e0516e37c89c09f1975dfc06b5870b1/lightning/src/ln/wire.rs#L21) with a method to read your custom message and return the enum.


## We will need to:
  - Define and implement (some traits for) all the custom messages we need.
  - Define an enum to hold all of our custom messages.
  - Define and implement a custom message handler.
<hr>

### Defining the Custom Messages
This will be like so:
```rust
use bitcoin::secp256k1::key::PublicKey;

// This is the register message.
#[derive(Debug)]
pub struct Register {
  pub pubkey: PublicKey,
  pub appointment_slots: u32,
  pub subscription_period: u32,
}

#[derive(Debug)]
pub struct AnotherCustomMsg {
  ...
}

// And other messages ...
```
The struct fields are the content of the message.

We will need to implement `Readable` and `Writeable` for our message:
```rust
use std::io;
use lightning::util::ser::{Readable, Writeable, Writer};

impl Readable for Register {
  fn read<R: io::Read>(reader: &mut R) -> Result<Self, DecodeError> {
    Ok(Self {
      pubkey: Readable::read(reader)?,
      appointment_slots: Readable::read(reader)?,
      subscription_period: Readable::read(reader)?,
    })
  }
}

impl Writeable for Register {
  fn write<W: Writer>(&self, writer: &mut W) -> Result<(), io::Error> {
    self.pubkey.write(writer)?;
    self.appointment_slots.write(writer)?;
    self.subscription_period.write(writer)?;
    Ok(())
  }
}
```
[`Readable`](https://github.com/lightningdevkit/rust-lightning/blob/9bdce47f0e0516e37c89c09f1975dfc06b5870b1/lightning/src/util/ser.rs#L205) is responsible for reading our message from raw bytes when it is received, while `Writeable` is responsible for writing our message as raw bytes so we can send it over the network to another node.

To implement `Readable` for your custom message, you will need to implement `Readable::read()` by just calling `Readable::read()` on each field of your message. Most probably, all your message field will already implement `Readable` (LDK has `Readable` and `Writeable` implementations for primitive types and commonly used types).

Writing is similar and is straightforward as illustrated in the code snippet above. Note that you should read and write message field in the same order, preferably, the order of their appearance in the custom message struct.
<hr>

### Defining an enum to hold the Custom Messages
This will be like so:
```rust
#[derive(Debug)]
pub enum CustomMessages {
  Register(Register),
  AnotherCustomMsg(AnotherCustomMsg),
  // Other msgs go here ...
}
```
This enum should list all the custom messages we want to use.

The enum should implement `Writeable` and `Type` LDK traits:
```rust
use std::io;
use lightning::util::ser::{Writeable, Writer};
use lightning::ln::wire::Type;

impl Writeable for CustomMessages {
  fn write<W: Writer>(&self, writer: &mut W) -> Result<(), io::Error> {
    match self {
      CustomMessages::Register(msg) => msg.write(writer),
      // You can have a specific message not implementing `Writeable`.
      // (e.g. you only receive this message but never send it, IOW read it but never write it)
      CustomMessages::AnotherCustomMsg(msg) => {
        Err(io::Error::new(io::ErrorKind::Unsupported, "This message is not writable"))
      }
    }
  }
}

impl Type for CustomMessages {
  fn type_id(&self) -> u16 {
    match self {
      // From https://github.com/lightning/bolts/blob/master/01-messaging.md#lightning-message-format
      // Custom (types 32768-65535): experimental and application-specific messages.
      CustomMessages::Register(..) => 37891,
      CustomMessages::AnotherCustomMsg(..) => 42900,
      // Other message types ...
    }
  }
}
```
The `Writeable` trait allows our custom messages to be written to a raw bytes buffer and the `Type` trait tags our message with a type (an `u16`) as every lightning message needs to have a type so the receiver knows how to decode the message from raw bytes.
<hr>

### Defining a Custom Message Handler
This will be like so:
```rust
use bitcoin::secp256k1::key::PublicKey;
use std::sync::Mutex;

pub struct OurCustomMessageHandler {
  /// This is a vector to hold the messages we are going to send to our peers.
  message_queue: Mutex<Vec<(PublicKey, CustomMessages)>>,
}
```
The custom message handler is a simple struct that will implement some LDK traits to make it ready to handle incoming messages. It might contain a local message queue if it is ought to send messages or respond to incoming messages.

Our custom message handler will need to implement the [`CustomMessageReader`](https://github.com/lightningdevkit/rust-lightning/blob/9bdce47f0e0516e37c89c09f1975dfc06b5870b1/lightning/src/ln/wire.rs#L21) trait:
```rust
use std::io;
use lightning::ln::wire::CustomMessageReader;

impl CustomMessageReader for OurCustomMessageHandler {
  type CustomMessage = CustomMessages;

  fn read<R: io::Read>(
    &self, message_type: u16, buffer: &mut R,
  ) -> Result<Option<TowerMessage>, DecodeError> {
    match message_type {
      37891 => Ok(Some(CustomMessages::Register(Readable::read(buffer)?))),
      42900 => {
        Ok(Some(CustomMessages::AnotherCustomMsg(Readable::read(buffer)?)))
      }
      // Unknown message.
      _ => Ok(None),
    }
  }
}
```
`CustomMessageReader::read()` will pass a message type and the raw bytes buffer to read the message from, and we should read the message with the proper format based on its type (the same types used when implementing the `Type` trait for `CustomMessages`).

Our custom message handler will also need to implement the [`CustomMessageHandler`](https://github.com/lightningdevkit/rust-lightning/blob/9bdce47f0e0516e37c89c09f1975dfc06b5870b1/lightning/src/ln/peer_handler.rs#L47) trait:
```rust
use bitcoin::secp256k1::key::PublicKey;
use core::mem;
use lightning::ln::peer_handler::CustomMessageHandler;
use lightning::ln::msgs::LightningError;

impl CustomMessageHandler for OurCustomMessageHandler {
  fn handle_custom_message(
    &self, msg: CustomMessages, sender_node_id: &PublicKey,
  ) -> Result<(), LightningError> {
    match msg {
      CustomMessages::Register(msg) => {
        // To send a response to the sender, create a new response message here
        // and add it to the local message queue to be scheduled for sending later.
        res = CustomMessages::RegisterAck {
          ...
        };
        self.message_queue.lock().unwrap().push((*sender_node_id, res));
      }
      _ => {
        // You can also return a lightning error or not take an action at all.
      }
    }
    Ok(())
  }

  fn get_and_clear_pending_msg(&self) -> Vec<(PublicKey, CustomMessages)> {
    std::mem::take(&mut self.message_queue.lock().unwrap())
  }
}
```
`CustomMessageHandler::handle_custom_message()` will be called when a custom message is received, and you will have the chance to act upon the message and maybe schedule a response to be sent by pushing it in the local message queue.
`CustomMessageHandler::get_and_clear_pending_msg()` is called periodically by the [`PeerManager`](https://github.com/lightningdevkit/rust-lightning/blob/9bdce47f0e0516e37c89c09f1975dfc06b5870b1/lightning/src/ln/peer_handler.rs#L413) to ask our custom message handler for the pending message in our message queue, these message could be either responses to incoming messages we handled in `handle_custom_message()` or could be just messages we decided to send by pushing them to the local message queue of the custom message handler. We could have something like the following to send messages proactively inside our application:
```rust
impl OurCustomMessageHandler {
  pub fn send_message(&self, pubkey: &PublicKey, msg: CustomMessages) {
    self.message_queue.lock().unwrap().push((*pubkey, msg))
  }
}
```

With this setup, our custom message handler is capable of sending and receiving the custom messages we defined. We still need to get the `PeerManager` to use our custom message handler.

Let's define a type similar to LDK's [`SimpleArcPeerManager`](https://github.com/lightningdevkit/rust-lightning/blob/9bdce47f0e0516e37c89c09f1975dfc06b5870b1/lightning/src/ln/peer_handler.rs#L382) but using `OurCustomMessageHandler` as the custom message handler instead of [`IgnoringMessageHandler`](https://github.com/lightningdevkit/rust-lightning/blob/9bdce47f0e0516e37c89c09f1975dfc06b5870b1/lightning/src/ln/peer_handler.rs#L61):
```rust
use lightning::ln::channelmanager::SimpleArcChannelManager;
use lightning::ln::peer_handler::PeerManager;
use lightning::routing::network_graph::{NetGraphMsgHandler, NetworkGraph};
use std::sync::Arc;

pub type SimpleArcPeerManagerWithOurCustomMessageHandler<SD, M, T, F, C, L> = PeerManager<
  SD,
  Arc<SimpleArcChannelManager<M, T, F, L>>,
  Arc<NetGraphMsgHandler<Arc<NetworkGraph>, Arc<C>, Arc<L>>>,
  Arc<L>,
  Arc<OurCustomMessageHandler>,
>;
```
Then define a peer manager type by filling concrete types in place of the generics `<SD, M, T, F, C, L>` like [this](https://github.com/lightningdevkit/ldk-sample/blob/c0a722430b8fbcb30310d64487a32aae839da3e8/src/main.rs#L90), but of course using `SimpleArcPeerManagerWithOurCustomMessageHandler` instead of `SimpleArcPeerManager`.

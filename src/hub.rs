use std::collections::HashMap;
use std::time::Duration;

use chrono::Utc;
use futures::future::err;
use futures::StreamExt;
use regex::Regex;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::{broadcast, RwLock};
use tokio::time;
use uuid::Uuid;

use crate::model::feed::Feed;
use crate::model::message::Message;
use crate::model::user::User;

use crate::proto::{
    Input, InputParcel, JoinInput, JoinedOutput, MessageOutput, Output, OutputError, OutputParcel,
    PostInput, PostedOutput, UserJoinedOutput, UserLeftOutput, UserOutput, UserPostedOutput,
};

const OUTPUT_CHANNEL_SIZE: usize = 16;

// HubOptions helps to separate domain-level options which could be read-in from and external
// configuration file in the future.
#[derive(Clone, Copy, Default)]
pub struct HubOptions {
    pub alive_interval: Option<Duration>,
}

pub struct Hub {
    alive_interval: Option<Duration>,
    output_sender: broadcast::Sender<OutputParcel>,
    // users and feed wrapped in RwLock because many concurrent tasks will access their values
    // and not necessarily modify them
    users: RwLock<HashMap<Uuid, User>>,
    feed: RwLock<Feed>,
}

impl Hub {
    pub fn new(options: HubOptions) -> Self {
        let (output_sender, _) = broadcast::channel(OUTPUT_CHANNEL_SIZE);
        Hub {
            alive_interval: options.alive_interval,
            output_sender,
            users: Default::default(),
            feed: Default::default(),
        }
    }

    async fn send(&self, output: Output) {
        if self.output_sender.receiver_count() == 0 {
            return;
        }
        self.users.read().await.keys().for_each(|user_id| {
            self.output_sender
                .send(OutputParcel::new(*user_id, output.clone()))
                .unwrap();
        });
    }

    // Send output to a specific user
    fn send_targeted(&self, client_id: Uuid, output: Output) {
        if self.output_sender.receiver_count() > 0 {
            self.output_sender
                .send(OutputParcel::new(client_id, output))
                .unwrap();
        }
    }

    // send to every user except one
    async fn send_ignored(&self, ignored_client_id: Uuid, output: Output) {
        if self.output_sender.receiver_count() == 0 {
            return;
        }
        self.users
            .read()
            .await
            .values()
            .filter(|user| user.id != ignored_client_id)
            .for_each(|user| {
                self.output_sender
                    .send(OutputParcel::new(user.id, output.clone()))
                    .unwrap();
            });
    }

    fn send_error(&self, client_id: Uuid, error: OutputError) {
        self.send_targeted(client_id, Output::Error(error));
    }

    // Listeners will be able to subscribe to hub's updates with subscribe - this will be used
    // to publish outputs to clients
    pub fn subscribe(&self) -> broadcast::Receiver<OutputParcel> {
        self.output_sender.subscribe()
    }

    pub async fn on_disconnect(&self, client_id:Uuid) {
        // Remove user on disconnect
        if self.users.write().await.remove(&client_id).is_some() {
            self.send_ignored(
                client_id,
                Output::UserLeft(UserLeftOutput::new(client_id)))
                .await;
        }
    }

    // tick_alive periodically sends Output::Alive messages to every user
    async fn tick_alive(&self) {
        let alive_interval = if let Some(alive_interval) = self.alive_interval {
            alive_interval
        } else {
            return;
        };

        loop {
            time::sleep(alive_interval).await;
            self.send(Output::Alive).await;
        }
    }
}
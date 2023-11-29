use std::sync::Arc;

use futures::{StreamExt, TryStreamExt};
use log::{error, info};
use tokio::signal;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::Duration;
use warp::Filter;
use warp::ws::WebSocket;

use crate::client::Client;
use crate::hub::{Hub, HubOptions};
use crate::proto::InputParcel;

pub struct Server {
    port: u16,
    hub: Arc<Hub>,
}

impl Server {
    pub fn new(port: u16) -> Self {
        Server {
            port,
            hub: Arc::new(Hub::new(HubOptions {
                alive_interval: Some(Duration::from_secs(5)),
            })),
        }
    }

    pub async fn run(&self) {
        let (input_sender, input_receiver) = mpsc::unbounded_channel::<InputParcel>();
        let hub = self.hub.clone();

        let feed = warp::path("feed")
            .and(warp::ws())
            .and(warp::any().map(move || input_sender.clone()))
            .and(warp::any().map(move || hub.clone()))
            .map(
                move
                    |ws: warp::ws::Ws,
                     input_sender: UnboundedSender<InputParcel>,
                     hub: Arc<hub>| {
                    ws.on_upgrade(move |web_socket| async move {
                        tokio::spawn(Self::process_client(hub, web_socket, input_sender));
                    })
                },
            );

        let shutdown = async {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to install CTRL+C handler");
        };

        let (_, serving) = warp::serve(feed)
            .bind_with_graceful_shutdown(([127, 0, 0, 1], self.port), shutdown);

        let running_hub = self.hub.run(input_receiver);

        tokio::select! {
            _ = serving => {},
            _ = running_hub => {},
        }
    }
}
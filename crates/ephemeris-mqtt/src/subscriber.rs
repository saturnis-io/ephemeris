use rumqttc::v5::mqttbytes::QoS;
use rumqttc::v5::{AsyncClient, EventLoop, MqttOptions};
use tracing::{error, info, warn};

use ephemeris_core::domain::EpcisEvent;
use ephemeris_core::repository::{AggregationRepository, EventRepository};

use crate::handler::EventHandler;

/// MQTT subscriber that listens for EPCIS events and routes them through the EventHandler.
pub struct MqttSubscriber {
    client: AsyncClient,
    eventloop: EventLoop,
}

impl MqttSubscriber {
    /// Create a new MQTT subscriber connected to the given broker.
    pub fn new(broker_host: &str, broker_port: u16, client_id: &str) -> Self {
        let mut options = MqttOptions::new(client_id, broker_host, broker_port);
        options.set_keep_alive(std::time::Duration::from_secs(30));

        let (client, eventloop) = AsyncClient::new(options, 100);
        Self { client, eventloop }
    }

    /// Subscribe to the given MQTT topics.
    pub async fn subscribe(&self, topics: &[String]) -> Result<(), rumqttc::v5::ClientError> {
        for topic in topics {
            self.client
                .subscribe(topic.clone(), QoS::AtLeastOnce)
                .await?;
            info!(topic = %topic, "subscribed to MQTT topic");
        }
        Ok(())
    }

    /// Run the event loop, processing incoming messages through the handler.
    ///
    /// This method runs indefinitely, polling the MQTT event loop and dispatching
    /// valid EPCIS event payloads to the handler.
    pub async fn run<E, A>(mut self, handler: EventHandler<E, A>)
    where
        E: EventRepository + 'static,
        A: AggregationRepository + 'static,
    {
        loop {
            match self.eventloop.poll().await {
                Ok(event) => {
                    if let rumqttc::v5::Event::Incoming(rumqttc::v5::Incoming::Publish(publish)) =
                        event
                    {
                        let payload = &publish.payload;
                        match serde_json::from_slice::<EpcisEvent>(payload) {
                            Ok(epcis_event) => {
                                if let Err(e) = handler.handle_event(&epcis_event).await {
                                    error!(error = %e, "failed to handle EPCIS event");
                                }
                            }
                            Err(e) => {
                                let topic = String::from_utf8_lossy(&publish.topic);
                                warn!(
                                    error = %e,
                                    topic = %topic,
                                    "invalid EPCIS payload, skipping"
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(error = %e, "MQTT connection error");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    }
}

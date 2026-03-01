use rumqttc::v5::mqttbytes::QoS;
use rumqttc::v5::{AsyncClient, EventLoop, MqttOptions};
use tracing::{error, info};

use ephemeris_core::domain::EpcisEvent;

/// Publishes EPCIS events to an MQTT broker for testing.
pub struct MqttPublisher {
    client: AsyncClient,
    eventloop: EventLoop,
    topic: String,
}

impl MqttPublisher {
    /// Create a new publisher targeting the given broker and topic.
    pub fn new(broker_host: &str, broker_port: u16, topic: &str) -> Self {
        let client_id = format!("ephemeris-testkit-{}", uuid::Uuid::new_v4());
        let mut options = MqttOptions::new(&client_id, broker_host, broker_port);
        options.set_keep_alive(std::time::Duration::from_secs(30));

        let (client, eventloop) = AsyncClient::new(options, 100);
        Self {
            client,
            eventloop,
            topic: topic.to_string(),
        }
    }

    /// Publish a single EPCIS event to the configured topic.
    pub async fn publish(&self, event: &EpcisEvent) -> Result<(), PublishError> {
        let payload = serde_json::to_vec(event).map_err(PublishError::Serialization)?;
        self.client
            .publish(&self.topic, QoS::AtLeastOnce, false, payload)
            .await
            .map_err(|e| PublishError::Client(Box::new(e)))?;
        Ok(())
    }

    /// Publish a batch of events with a delay between each.
    pub async fn publish_batch(
        &self,
        events: &[EpcisEvent],
        delay: std::time::Duration,
    ) -> Result<usize, PublishError> {
        let mut sent = 0;
        for event in events {
            self.publish(event).await?;
            sent += 1;
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
        }
        info!(count = sent, topic = %self.topic, "published event batch");
        Ok(sent)
    }

    /// Drive the MQTT event loop. Must be polled concurrently with publish calls.
    ///
    /// Runs indefinitely, handling MQTT protocol traffic (CONNACK, PUBACK, etc.).
    pub async fn run_eventloop(mut self) {
        loop {
            match self.eventloop.poll().await {
                Ok(_) => {}
                Err(e) => {
                    error!(error = %e, "MQTT publisher eventloop error");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    }
}

/// Errors from the MQTT publisher.
#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("MQTT client error: {0}")]
    Client(#[from] Box<rumqttc::v5::ClientError>),
}

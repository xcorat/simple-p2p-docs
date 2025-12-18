use libp2p::gossipsub::{self, IdentTopic, MessageAuthenticity, MessageId};
use libp2p::identity::Keypair;
use std::time::Duration;

/// Helper to construct a gossipsub behaviour configured for the docstore topic(s).
pub fn make_docstore_gossipsub(local_key: &Keypair) -> gossipsub::Behaviour {
    let config = gossipsub::ConfigBuilder::default()
        .validation_mode(gossipsub::ValidationMode::Strict)
        .heartbeat_interval(Duration::from_secs(1))
        .build()
        .expect("valid gossipsub config");

    gossipsub::Behaviour::new(MessageAuthenticity::Signed(local_key.clone()), config)
        .expect("gossipsub")
}

/// Topic used for public document updates
pub fn docstore_topic() -> IdentTopic {
    IdentTopic::new("docstore/v1/updates")
}

/// Subscribe the provided gossipsub behaviour to the docstore topic.
pub fn subscribe(beh: &mut gossipsub::Behaviour) -> anyhow::Result<()> {
    beh.subscribe(&docstore_topic()).map(|_b| ()).map_err(|e| anyhow::anyhow!(e))
}

/// Publish data to the docstore topic using the given gossipsub behaviour.
pub fn publish_update(
    beh: &mut gossipsub::Behaviour,
    data: impl Into<Vec<u8>>,
) -> Result<MessageId, gossipsub::PublishError> {
    beh.publish(docstore_topic(), data.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity::Keypair;

    #[test]
    fn test_subscribe_and_publish() {
        let key = Keypair::generate_ed25519();
        let mut beh = make_docstore_gossipsub(&key);
        assert!(subscribe(&mut beh).is_ok());
        let res = publish_update(&mut beh, b"hello world".to_vec());
        assert!(res.is_ok());
    }
}

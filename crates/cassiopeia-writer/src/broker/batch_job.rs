use cassiopeia_ngsi_ld::entity::NgsiLdEntity;

/// A batch of entities handed from the main thread to a sender worker over the channel.
pub struct BatchJob {
    /// The entities to serialize and POST as one request.
    pub entities: Vec<NgsiLdEntity>,
}

#[cfg(test)]
mod tests {
    use crate::broker::batch_job::BatchJob;
    use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, name::NameBuf};
    use urn_rs::Urn;

    #[test]
    fn a_job_carries_the_entities_it_was_built_with() {
        let job = BatchJob {
            entities: vec![NgsiLdEntity::new(
                "urn:ngsi-ld:Sensor:1".parse::<Urn>().unwrap(),
                NameBuf::new("Sensor").unwrap(),
            )],
        };

        assert_eq!(job.entities.len(), 1);
    }
}

use crate::error::{PipelineError, Result};
use cassiopeia_common::{batch::Batch, channel::ChannelReceiver, signal::Signal};

/// Drains a stage's completion channel, propagating the first error a stage reported.
///
/// Both pipeline phases end on this loop, so a failed input phase and a failed output phase abort
/// the run the same way: [`Signal::Stop`] ends the stream cleanly, while a [`Signal::Error`] is
/// returned, so the cycle (and through it the run's exit status) records the failure rather than
/// swallowing it. Every other signal is control traffic that carries no completion payload.
///
/// Nothing is reported here. A fatal failure is reported exactly once, at the top of the process,
/// where it can be rendered with its whole cause chain. Reporting it here as well would print it
/// twice, and since this path only ever had the rendered string, it would print the shallower of
/// the two.
///
/// # Errors
///
/// Returns the [`PipelineError`] carried by the first [`Signal::Error`] drained from `rx`.
pub(crate) fn drain_completion(rx: ChannelReceiver<Signal<Batch<()>, PipelineError>>) -> Result<()> {
    for signal in rx {
        match signal {
            Signal::Stop => break,
            Signal::Error(error) => return Err(error),
            // A sink stage carries no data payload; its channel exists only for these control
            // signals, so a data batch on it is empty and means nothing.
            Signal::Start | Signal::Meta(_) | Signal::Data(_) => {}
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{completion::drain_completion, error::PipelineError, pipeline_stage::PipelineStage};
    use cassiopeia_common::{
        batch::Batch,
        channel::{ChannelPolicy, channel_with_telemetry},
        signal::{MetaSignal, Signal},
    };

    #[test]
    fn an_error_signal_is_propagated_as_an_error() {
        let (tx, rx) = channel_with_telemetry::<Signal<Batch<()>, PipelineError>>(ChannelPolicy::Unbounded, None, None);
        tx.send(Signal::Error(PipelineError::StagePanic { stage: PipelineStage::Writer })).unwrap();
        drop(tx);

        let result = drain_completion(rx);

        assert!(matches!(result, Err(PipelineError::StagePanic { stage: PipelineStage::Writer })));
    }

    #[test]
    fn a_stop_after_data_signals_completes_cleanly() {
        let (tx, rx) = channel_with_telemetry::<Signal<Batch<()>, PipelineError>>(ChannelPolicy::Unbounded, None, None);
        tx.send(Signal::Data(Batch::default())).unwrap();
        tx.send(Signal::Data(Batch::default())).unwrap();
        tx.send(Signal::Stop).unwrap();
        drop(tx);

        assert!(drain_completion(rx).is_ok());
    }

    #[test]
    fn start_and_meta_signals_are_ignored() {
        let (tx, rx) = channel_with_telemetry::<Signal<Batch<()>, PipelineError>>(ChannelPolicy::Unbounded, None, None);
        tx.send(Signal::Start).unwrap();
        tx.send(Signal::Meta(MetaSignal::InputCount(1))).unwrap();
        tx.send(Signal::Stop).unwrap();
        drop(tx);

        assert!(drain_completion(rx).is_ok());
    }
}

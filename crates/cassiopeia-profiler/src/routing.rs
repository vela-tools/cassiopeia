use crate::{error::ProfilerError, profiler::ProfilerRoutes};
use cassiopeia_common::{channel::ChannelReceiver, signal::Signal, stage::Stage, telemetry::run::RunTelemetry};
use cassiopeia_data_profiler::profile::Profile;
use cassiopeia_ir::{
    payload::{CollectedPayload, ProfiledPayload},
    payload_origin::PayloadOrigin,
};
use std::{convert::Infallible, time::Instant};

/// Drives one profiler to completion: for each collected payload it derives a [`Profile`] via
/// `profile_of`, then routes the resulting [`ProfiledPayload`] to the channel for that format.
///
/// The two profilers differ only in how a payload's profile is produced (content detection versus
/// a declared format), so the receiver loop and the routing live here once instead of in each
/// implementation. The input carries no error variant ([`Infallible`]): upstream failures are
/// routed to the pipeline's error path before they reach the profiler, so the profiler only ever
/// raises its own [`ProfilerError`].
pub(crate) fn route_payloads(
    receiver: &ChannelReceiver<Signal<CollectedPayload, Infallible>>,
    routes: ProfilerRoutes,
    mut profile_of: impl FnMut(&CollectedPayload) -> Result<Profile, ProfilerError>,
    telemetry: &RunTelemetry,
) -> Result<(), ProfilerError> {
    let stage = telemetry.start_stage(Stage::Profiler);
    // An explicit receive rather than the channel's iterator, so the wait for the next payload is
    // measured as this stage's input wait instead of vanishing into the loop header.
    while let Ok(signal) = stage.measure_input_wait(|| receiver.recv()) {
        match signal {
            Signal::Data(payload) => {
                stage.received(1);
                let service = stage.service_span();
                let profile = match profile_of(&payload) {
                    Ok(profile) => profile,
                    Err(error) => {
                        stage.failed(1);
                        return Err(error);
                    }
                };
                drop(service);
                let format = *profile.format();
                let Some(route) = routes.get(&format) else {
                    stage.failed(1);
                    return Err(ProfilerError::NoRoute {
                        format,
                        origin: PayloadOrigin::of(&payload),
                    });
                };
                let send_started = Instant::now();
                let send_result = route.send(Signal::Data(ProfiledPayload::new(payload, profile)));
                stage.add_output_wait(send_started.elapsed());
                send_result.map_err(|_| ProfilerError::ChannelClosed)?;
                stage.completed(1);
            }
            Signal::Stop => break,
            Signal::Error(never) => match never {},
            Signal::Start | Signal::Meta(_) => {}
        }
    }

    // Dropping the routes closes every ingestor input channel, signalling end-of-stream.
    drop(routes);
    Ok(())
}

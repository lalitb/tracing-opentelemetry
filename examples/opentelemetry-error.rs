use std::{
    error::Error as StdError,
    fmt::{Debug, Display},
    io::Write,
    thread,
    time::{Duration, SystemTime},
};

use opentelemetry::{global, trace::TracerProvider};
use opentelemetry_sdk::Resource;

use opentelemetry_sdk::{
    self as sdk,
    export::trace::{ExportResult, SpanExporter},
};
use tracing::{error, instrument, span, trace, warn};
use tracing_subscriber::prelude::*;

#[derive(Debug)]
enum Error {
    ErrorQueryPassed,
}

impl StdError for Error {}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::ErrorQueryPassed => write!(f, "Encountered the error flag in the query"),
        }
    }
}

#[instrument(err)]
fn failable_work(fail: bool) -> Result<&'static str, Error> {
    span!(tracing::Level::INFO, "expensive_step_1")
        .in_scope(|| thread::sleep(Duration::from_millis(25)));
    span!(tracing::Level::INFO, "expensive_step_2")
        .in_scope(|| thread::sleep(Duration::from_millis(25)));

    if fail {
        return Err(Error::ErrorQueryPassed);
    }
    Ok("success")
}

#[instrument(err)]
fn double_failable_work(fail: bool) -> Result<&'static str, Error> {
    span!(tracing::Level::INFO, "expensive_step_1")
        .in_scope(|| thread::sleep(Duration::from_millis(25)));
    span!(tracing::Level::INFO, "expensive_step_2")
        .in_scope(|| thread::sleep(Duration::from_millis(25)));
    error!(error = "test", "hello");
    if fail {
        return Err(Error::ErrorQueryPassed);
    }
    Ok("success")
}

fn main() -> Result<(), Box<dyn StdError + Send + Sync + 'static>> {
    let builder = sdk::trace::TracerProvider::builder().with_simple_exporter(WriterExporter{resource: Resource::default()});
    let provider = builder.build();
    let tracer = provider
        .tracer_builder("opentelemetry-write-exporter")
        .build();
    global::set_tracer_provider(provider);

    let opentelemetry = tracing_opentelemetry::layer().with_tracer(tracer);
    tracing_subscriber::registry()
        .with(opentelemetry)
        .try_init()?;

    {
        let root = span!(tracing::Level::INFO, "app_start", work_units = 2);
        let _enter = root.enter();

        let work_result = failable_work(false);

        trace!("status: {}", work_result.unwrap());
        let work_result = failable_work(true);

        trace!("status: {}", work_result.err().unwrap());
        warn!("About to exit!");

        let _ = double_failable_work(true);
    } // Once this scope is closed, all spans inside are closed as well

    // Shut down the current tracer provider. This will invoke the shutdown
    // method on all span processors. span processors should export remaining
    // spans before return.
    global::shutdown_tracer_provider();

    Ok(())
}

#[derive(Debug)]
struct WriterExporter{
    // set exporter
    resource: Resource
}

impl SpanExporter for WriterExporter {
    fn export(
        &mut self,
        batch: Vec<sdk::export::trace::SpanData>,
    ) -> futures_util::future::BoxFuture<'static, sdk::export::trace::ExportResult> {
        let mut writer = std::io::stdout();
        for span in batch {
            let span_data = SpanData {span: span, resource: self.resource.clone()};
            writeln!(writer, "{}", span_data).unwrap();
        }
        writeln!(writer).unwrap();

        Box::pin(async move { ExportResult::Ok(()) })
    }

    fn set_resource(&mut self, resource: &opentelemetry_sdk::Resource) {
        self.resource = resource.clone();
        
    }
}

struct SpanData{
    span: sdk::export::trace::SpanData,
    resource: Resource,
}
impl Display for SpanData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Span: \"{}\"", self.span.name)?;
        match &self.span.status {
            opentelemetry::trace::Status::Unset => {}
            opentelemetry::trace::Status::Error { description } => {
                writeln!(f, "- Status: Error")?;
                writeln!(f, "- Error: {description}")?
            }
            opentelemetry::trace::Status::Ok => writeln!(f, "- Status: Ok")?,
        }
        writeln!(
            f,
            "- Start: {}",
            self.span
                .start_time
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("start time is before the unix epoch")
                .as_secs()
        )?;
        writeln!(
            f,
            "- End: {}",
            self.span
                .end_time
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("end time is before the unix epoch")
                .as_secs()
        )?;
        writeln!(f, "- Resource:")?;
        for (k, v) in self.resource.iter() {
            writeln!(f, "  - {}: {}", k, v)?;
        }
        writeln!(f, "- Attributes:")?;
        for kv in self.span.attributes.iter() {
            writeln!(f, "  - {}: {}", kv.key, kv.value)?;
        }

        writeln!(f, "- Events:")?;
        for event in self.span.events.iter() {
            if let Some(error) =
                event
                    .attributes
                    .iter()
                    .fold(Option::<String>::None, |mut acc, d| {
                        if let Some(mut acc) = acc.take() {
                            use std::fmt::Write;
                            let _ = write!(acc, ", {}={}", d.key, d.value);
                            Some(acc)
                        } else {
                            Some(format!("{} = {}", d.key, d.value))
                        }
                    })
            {
                writeln!(f, "  - \"{}\" {{{error}}}", event.name)?;
            } else {
                writeln!(f, "  - \"{}\"", event.name)?;
            }
        }
        writeln!(f, "- Links:")?;
        for link in self.span.links.iter() {
            writeln!(f, "  - {:?}", link)?;
        }
        Ok(())
    }
}

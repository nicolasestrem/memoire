//! Colored logging infrastructure for component identification
//!
//! Provides a custom tracing formatter that adds colored prefixes to distinguish
//! output from different components (recorder, indexers, viewer, etc.).

use owo_colors::{OwoColorize, Style};
use std::fmt;
use std::io;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::{
    format::{Writer, FormatEvent, FormatFields},
    FmtContext,
};
use tracing_subscriber::registry::LookupSpan;

/// Component identifier for prefixing logs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    Orchestrator,
    Record,
    Index,
    AudioIndex,
    Viewer,
    Models,
}

impl Component {
    /// Get the string representation for logging prefix
    pub fn as_str(&self) -> &'static str {
        match self {
            Component::Orchestrator => "MAIN",
            Component::Record => "RECORD",
            Component::Index => "INDEX",
            Component::AudioIndex => "AUDIO",
            Component::Viewer => "VIEWER",
            Component::Models => "MODELS",
        }
    }

    /// Get the color style for this component
    pub fn color_style(&self) -> Style {
        match self {
            Component::Orchestrator => Style::new().cyan().bold(),
            Component::Record => Style::new().green().bold(),
            Component::Index => Style::new().yellow().bold(),
            Component::AudioIndex => Style::new().magenta().bold(),
            Component::Viewer => Style::new().blue().bold(),
            Component::Models => Style::new().white().bold(),
        }
    }
}

/// Custom formatter with component prefixes and colors
pub struct ColoredFormatter {
    pub component: Component,
}

impl<S, N> FormatEvent<S, N> for ColoredFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let metadata = event.metadata();

        // Timestamp (HH:MM:SS format)
        let now = chrono::Local::now();
        write!(writer, "{} ", now.format("%H:%M:%S").dimmed())?;

        // Component prefix with color
        let prefix = format!("[{:8}]", self.component.as_str());
        write!(writer, "{} ", prefix.style(self.component.color_style()))?;

        // Log level with appropriate color
        let level = metadata.level();
        match *level {
            Level::ERROR => write!(writer, "{} ", "ERROR".red().bold())?,
            Level::WARN => write!(writer, "{} ", "WARN ".yellow().bold())?,
            Level::INFO => write!(writer, "{} ", "INFO ".green().bold())?,
            Level::DEBUG => write!(writer, "{} ", "DEBUG".blue().bold())?,
            Level::TRACE => write!(writer, "{} ", "TRACE".dimmed().bold())?,
        }

        // Message content
        ctx.field_format().format_fields(writer.by_ref(), event)?;

        writeln!(writer)
    }
}

/// Initialize colored logging for a specific component
///
/// This sets up a tracing subscriber with colored output.
/// Should be called once per component/process.
pub fn init_component_logger(component: Component) -> anyhow::Result<()> {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    let fmt_layer = tracing_subscriber::fmt::layer()
        .event_format(ColoredFormatter { component })
        .with_writer(io::stdout);

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive(tracing::Level::INFO.into()))
        .with(fmt_layer)
        .try_init()?;

    Ok(())
}

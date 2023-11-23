#[macro_export]
macro_rules! init_logger {
    ($t: tt) => {
        use tracing_subscriber::{
            fmt::writer::MakeWriterExt, layer::SubscriberExt, util::SubscriberInitExt, Layer,
        };
        let file_appender = tracing_appender::rolling::daily($t, "app.log");
        let (file_non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
        let stdout = std::io::stdout.with_max_level(tracing::Level::INFO);
        let time_format =
            time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
        let timer = tracing_subscriber::fmt::time::UtcTime::new(time_format);
        let format = tracing_subscriber::fmt::format()
            .with_level(true)
            .with_target(false)
            .with_thread_ids(false)
            .with_thread_names(false);
        let mut layers = Vec::new();
        let file_layer = tracing_subscriber::fmt::layer()
            .event_format(format.clone().json())
            .with_timer(timer.clone())
            .with_writer(file_non_blocking)
            .with_filter(tracing_subscriber::filter::LevelFilter::INFO)
            .boxed();
        layers.push(file_layer);
        let console_layer = tracing_subscriber::fmt::layer()
            .event_format(format.clone().compact())
            .with_timer(timer)
            .with_writer(stdout)
            .with_filter(tracing_subscriber::filter::LevelFilter::INFO)
            .boxed();
        layers.push(console_layer);
        tracing_subscriber::registry().with(layers).init();
    };
}

//! Логи работы программы: неблокирующая запись в `log_out/` (крейт `tracing-appender`).

use std::path::Path;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

const LOG_DIR: &str = "log_out";
const LOG_FILE_PREFIX: &str = "from-kafka-to-iceberg";

/// Подключает `tracing` к ротируемому файлу в `log_out/` через очередь в отдельном потоке.
///
/// Возвращаемый [`WorkerGuard`] нужно удерживать до выхода из процесса — при дропе воркер
/// корректно сбрасывает буфер и завершается.
pub fn init() -> WorkerGuard {
    if let Err(e) = std::fs::create_dir_all(LOG_DIR) {
        eprintln!("не удалось создать каталог {LOG_DIR}: {e}");
    }

    let file_appender = RollingFileAppender::new(Rotation::DAILY, Path::new(LOG_DIR), LOG_FILE_PREFIX);
    let (writer, guard) = NonBlocking::new(file_appender);

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .with_writer(writer)
                .with_ansi(false)
                .with_target(true)
                .with_line_number(true),
        )
        .init();

    guard
}

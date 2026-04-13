use serde::{Deserialize, Serialize};

mod env_work;
mod iceberg;
mod kafka;
mod my_log;

/// Одна запись из лога после разбора.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: char,
    pub component: String,
    pub message: String,
}

#[tokio::main]
async fn main() {
    env_work::ensure_dotenv_loaded();
    let _log_guard = my_log::init();
    let _ = kafka::kafka_brokers_and_topic_mix();

    match kafka::read_log_entries_from_kafka() {
        Ok(entries) => {
            tracing::info!(count = entries.len(), "прочитано записей из Kafka");
            if let Err(e) = iceberg::send_log_entries_to_iceberg(&entries).await {
                tracing::error!(error = %e, "запись в Iceberg не удалась");
                eprintln!("Ошибка записи в Iceberg: {e}");
                std::process::exit(1);
            }
            // println!("Hello, world! записей: {}", entries.len());
        }
        Err(e) => {
            tracing::error!(error = %e, "чтение из Kafka не удалось");
            eprintln!("Ошибка чтения из Kafka: {e}");
            std::process::exit(1);
        }
    }
}

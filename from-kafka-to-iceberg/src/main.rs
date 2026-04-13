//! # from-kafka-to-iceberg
//!
//! Консольное приложение: **чтение** сообщений из **Apache Kafka** (JSON → [`LogEntry`]) и **запись**
//! батча в **Apache Iceberg** (Parquet, in-memory каталог, локальный warehouse). Служебные логи — в
//! каталоге `log_out/` ([`my_log`]).
//!
//! ## Запуск
//!
//! ```text
//! cargo run
//! ```
//!
//! Переменные окружения и пример `.env` описаны в файле **README.md** в корне репозитория.
//! Kafka: [`env_work::KAFKA_BROKERS`], [`env_work::KAFKA_TOPIC`], таймауты — [`kafka::KAFKA_TIMEOUT`],
//! [`kafka::KAFKA_CONNECT_TIMEOUT`]. Iceberg warehouse: [`env_work::ICEBERG_WAREHOUSE`].
//!
//! ## Документация API (rustdoc)
//!
//! ```text
//! cargo doc --no-deps --open
//! ```
//!
//! Откроется HTML со страницей крейта и ссылками на публичные элементы (см. реэкспорты ниже и подмодули).
//!
//! ## Модули
//!
//! | Модуль | Назначение |
//! |--------|------------|
//! | [`env_work`] | Загрузка `.env`, [`env_work::KAFKA_BROKERS`], [`env_work::KAFKA_TOPIC`], [`env_work::ICEBERG_WAREHOUSE`] |
//! | [`my_log`] | Инициализация `tracing`, ротация файлов в `log_out/` |
//! | [`kafka`] | Consumer, [`kafka::read_log_entries_from_kafka`], таймауты, [`kafka::kafka_brokers_and_topic_mix`] |
//! | [`iceberg`] | [`iceberg::send_log_entries_to_iceberg`], схема таблицы `logging.log_entries` |

use serde::{Deserialize, Serialize};

mod env_work;
mod iceberg;
mod kafka;
mod my_log;

pub use env_work::{
    ensure_dotenv_loaded, ICEBERG_WAREHOUSE, KAFKA_BROKERS, KAFKA_TOPIC,
};
pub use iceberg::send_log_entries_to_iceberg;
pub use kafka::{
    kafka_brokers_and_topic_mix, read_log_entries_from_kafka, KAFKA_CONNECT_TIMEOUT, KAFKA_TIMEOUT,
    ReadLogEntriesError,
};

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

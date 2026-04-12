//! Загрузка переменных из файла `.env` (крейт `dotenv`).

use std::sync::{LazyLock, Once};

static DOTENV_LOADED: Once = Once::new();

fn load_dotenv() {
    DOTENV_LOADED.call_once(|| {
        dotenv::dotenv().ok();
    });
}

/// Однократная загрузка `.env` (для модулей вроде `kafka`, где нужны переменные после `dotenv`).
pub fn ensure_dotenv_loaded() {
    load_dotenv();
}

/// Имя переменной окружения для списка брокеров Kafka.
const KAFKA_BROKERS_ENV: &str = "KAFKA_BROKERS";

/// Значение `KAFKA_BROKERS` из `.env` или окружения процесса.
pub static KAFKA_BROKERS: LazyLock<String> = LazyLock::new(|| {
    load_dotenv();
    let key = KAFKA_BROKERS_ENV;
    match std::env::var(key) {
        Ok(value) => value,
        Err(e) => {
            tracing::error!(
                error = %e,
                key = key,
                "{key} не задан (ни в .env, ни в окружении)"
            );
            panic!("{key} не задан (ни в .env, ни в окружении)");
        }
    }
});

/// Имя переменной окружения для топика Kafka.
const KAFKA_TOPIC_ENV: &str = "KAFKA_TOPIC";

/// Значение `KAFKA_TOPIC` из `.env` или окружения процесса.
pub static KAFKA_TOPIC: LazyLock<String> = LazyLock::new(|| {
    load_dotenv();
    let key = KAFKA_TOPIC_ENV;
    match std::env::var(key) {
        Ok(value) => value,
        Err(e) => {
            tracing::error!(
                error = %e,
                key = key,
                "{key} не задан (ни в .env, ни в окружении)"
            );
            panic!("{key} не задан (ни в .env, ни в окружении)");
        }
    }
});


//! Интеграция с **Apache Kafka** через [`rdkafka`]: асинхронный продюсер, JSON-сообщения и
//! таймауты из переменных окружения (через [`crate::env_work`] и локальные [`LazyLock`]-статики).
//!
//! # Настройка
//!
//! Адрес брокеров и имя топика задаются в модуле [`env_work`] (`KAFKA_BROKERS`, `KAFKA_TOPIC`).
//! В этом модуле дополнительно читаются:
//!
//! - [`KAFKA_TIMEOUT`] — максимальное время ожидания доставки одного сообщения продюсером
//!   ([`FutureProducer::send`](rdkafka::producer::FutureProducer::send)), в **секундах**;
//! - [`KAFKA_CONNECT_TIMEOUT`] — время на установку соединения с брокером, в **секундах**; передаётся
//!   в клиент как `socket.connection.setup.timeout.ms` (миллисекунды).
//!
//! Перед первым чтением переменных вызывается [`env_work::ensure_dotenv_loaded`], чтобы учесть файл `.env`.
//!
//! # Отправка данных
//!
//! Функция [`send_users_to_kafka`] сериализует срез [`crate::UserRow`] в JSON-массив
//! (`serde_json`) и отправляет **одним** сообщением в указанный топик. Ключ сообщения не задаётся
//! (тип ключа `()` в [`FutureRecord`]).
//!
//! # Ошибки и логирование
//!
//! При отсутствии переменных окружения или неверном формате числа инициализация статиков завершается
//! паникой после записи события в лог через `tracing` (если подписчик уже инициализирован в [`crate::my_log`]).
use crate::env_work;
use crate::UserRow;
use rdkafka::config::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use std::sync::LazyLock;
use std::time::Duration;

/// Имя переменной окружения для таймаута операций с Kafka (секунды).
const KAFKA_TIMEOUT_ENV: &str = "KAFKA_TIMEOUT";

/// Таймаут операций с Kafka в секундах (`KAFKA_TIMEOUT` из `.env` или окружения).
pub static KAFKA_TIMEOUT: LazyLock<u64> = LazyLock::new(|| {
    env_work::ensure_dotenv_loaded();
    let key = KAFKA_TIMEOUT_ENV;
    let raw = match std::env::var(key) {
        Ok(value) => value,
        Err(e) => {
            tracing::error!(
                error = %e,
                key = key,
                "{key} не задан (ни в .env, ни в окружении)"
            );
            panic!("{key} не задан (ни в .env, ни в окружении)");
        }
    };
    match raw.parse::<u64>() {
        Ok(n) => n,
        Err(e) => {
            tracing::error!(
                error = %e,
                key = key,
                value = %raw,
                "{key} должен быть целым числом секунд"
            );
            panic!("{key} должен быть целым числом секунд: {e}");
        }
    }
});

/// Имя переменной окружения для таймаута установки соединения с брокером (секунды).
const KAFKA_CONNECT_TIMEOUT_ENV: &str = "KAFKA_CONNECT_TIMEOUT";

/// Таймаут установки соединения с брокером в секундах (`KAFKA_CONNECT_TIMEOUT` из `.env` или окружения).
pub static KAFKA_CONNECT_TIMEOUT: LazyLock<u64> = LazyLock::new(|| {
    env_work::ensure_dotenv_loaded();
    let key = KAFKA_CONNECT_TIMEOUT_ENV;
    let raw = match std::env::var(key) {
        Ok(value) => value,
        Err(e) => {
            tracing::error!(
                error = %e,
                key = key,
                "{key} не задан (ни в .env, ни в окружении)"
            );
            panic!("{key} не задан (ни в .env, ни в окружении)");
        }
    };
    match raw.parse::<u64>() {
        Ok(n) => n,
        Err(e) => {
            tracing::error!(
                error = %e,
                key = key,
                value = %raw,
                "{key} должен быть целым числом секунд"
            );
            panic!("{key} должен быть целым числом секунд: {e}");
        }
    }
});

/// Возвращает клоны строк брокеров и топика из [`env_work`] и значение [`KAFKA_TIMEOUT`].
///
/// Третий элемент кортежа — таймаут отправки в секундах (для вызова из `main` часто игнорируют
/// через `let (a, b, ..)`, так как тот же таймаут используется внутри [`send_users_to_kafka`]
/// через статик [`KAFKA_TIMEOUT`]).
pub fn kafka_brokers_and_topic_mix() -> (String, String, u64) {
    (
        env_work::KAFKA_BROKERS.clone(),
        env_work::KAFKA_TOPIC.clone(),
        *KAFKA_TIMEOUT,
    )
}

/// Сериализует `v` в JSON-массив и отправляет **одним** сообщением в Kafka.
///
/// `brokers` и `topic` обычно берут из [`kafka_brokers_and_topic_mix`] или из статиков [`env_work`].
/// Таймауты берутся из [`KAFKA_CONNECT_TIMEOUT`] (соединение) и [`KAFKA_TIMEOUT`] (ожидание send).
pub async fn send_users_to_kafka(
    brokers: &str,
    topic: &str,
    v: &[UserRow],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let payload = serde_json::to_string(v)?;
    let connect_timeout_ms = (*KAFKA_CONNECT_TIMEOUT).saturating_mul(1000);
    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", brokers)
        .set(
            "socket.connection.setup.timeout.ms",
            connect_timeout_ms.to_string(),
        )
        .create()?;
    producer
        .send(
            FutureRecord::<(), str>::to(topic).payload(payload.as_str()),
            Duration::from_secs(*KAFKA_TIMEOUT),
        )
        .await
        .map_err(|(e, _)| e)?;
    Ok(())
}


//! Параметры подключения к **Apache Kafka** из `.env`: брокеры, топик и таймауты
//! (через [`crate::env_work`] и локальные [`LazyLock`]-статики).
//!
//! # Настройка
//!
//! Адрес брокеров и имя топика задаются в модуле [`env_work`] (`KAFKA_BROKERS`, `KAFKA_TOPIC`).
//! Путь warehouse для Iceberg — [`env_work::ICEBERG_WAREHOUSE`]; при вызове [`kafka_brokers_and_topic_mix`]
//! он тоже инициализируется, чтобы при старте конвейера Kafka → Iceberg все нужные переменные были проверены.
//! В этом модуле дополнительно читаются:
//!
//! - [`KAFKA_TIMEOUT`] — таймаут операций с Kafka в **секундах** (переменная `KAFKA_TIMEOUT`);
//! - [`KAFKA_CONNECT_TIMEOUT`] — таймаут установки соединения с брокером в **секундах**; при настройке
//!   клиента передаётся как `socket.connection.setup.timeout.ms` (миллисекунды).
//!
//! Перед первым чтением переменных вызывается [`env_work::ensure_dotenv_loaded`], чтобы учесть файл `.env`.
//!
//! Функция [`kafka_brokers_and_topic_mix`] возвращает строки брокеров, топика и значение [`KAFKA_TIMEOUT`]
//! и при вызове инициализирует также [`KAFKA_CONNECT_TIMEOUT`] (оба таймаута проверяются по `.env`).
//!
//! Функция [`read_log_entries_from_kafka`] подписывается на топик [`env_work::KAFKA_TOPIC`], читает
//! сообщения с полезной нагрузкой в JSON (один [`crate::LogEntry`] или массив записей) и возвращает
//! [`Vec`] за время не дольше [`KAFKA_TIMEOUT`] секунд; таймаут также задаёт `socket.timeout.ms` у consumer.
//!
//! # Ошибки и логирование
//!
//! При отсутствии переменных окружения или неверном формате числа инициализация статиков завершается
//! паникой после записи события в лог через `tracing` (если подписчик уже инициализирован в [`crate::my_log`]).
use crate::env_work;
use crate::LogEntry;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, BaseConsumer};
use rdkafka::Message;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

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
/// Третий элемент кортежа — таймаут в секундах ([`KAFKA_TIMEOUT`]); при необходимости его можно не
/// связывать с именем: `let (brokers, topic, ..) = kafka_brokers_and_topic_mix()`.
/// [`KAFKA_CONNECT_TIMEOUT`] и [`env_work::ICEBERG_WAREHOUSE`] читаются при этом же вызове
/// (warehouse — для последующей записи в Iceberg).
pub fn kafka_brokers_and_topic_mix() -> (String, String, u64) {
    let _ = *KAFKA_CONNECT_TIMEOUT;
    let _ = env_work::ICEBERG_WAREHOUSE.clone();
    (
        env_work::KAFKA_BROKERS.clone(),
        env_work::KAFKA_TOPIC.clone(),
        *KAFKA_TIMEOUT,
    )
}

/// Ошибка чтения и разбора сообщений из Kafka.
#[derive(Debug)]
pub enum ReadLogEntriesError {
    Kafka(rdkafka::error::KafkaError),
    Json(serde_json::Error),
}

impl std::fmt::Display for ReadLogEntriesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadLogEntriesError::Kafka(e) => write!(f, "Kafka: {e}"),
            ReadLogEntriesError::Json(e) => write!(f, "JSON: {e}"),
        }
    }
}

impl std::error::Error for ReadLogEntriesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ReadLogEntriesError::Kafka(e) => Some(e),
            ReadLogEntriesError::Json(e) => Some(e),
        }
    }
}

impl From<rdkafka::error::KafkaError> for ReadLogEntriesError {
    fn from(e: rdkafka::error::KafkaError) -> Self {
        ReadLogEntriesError::Kafka(e)
    }
}

impl From<serde_json::Error> for ReadLogEntriesError {
    fn from(e: serde_json::Error) -> Self {
        ReadLogEntriesError::Json(e)
    }
}

fn parse_json_payload_to_entries(payload: &[u8]) -> Result<Vec<LogEntry>, serde_json::Error> {
    if let Ok(entries) = serde_json::from_slice::<Vec<LogEntry>>(payload) {
        return Ok(entries);
    }
    serde_json::from_slice::<LogEntry>(payload).map(|e| vec![e])
}

/// Читает сообщения из топика [`env_work::KAFKA_TOPIC`]: подключение к [`env_work::KAFKA_BROKERS`],
/// уникальная consumer group на вызов, `auto.offset.reset=earliest`.
///
/// Полезная нагрузка — JSON: один объект [`LogEntry`] или JSON-массив таких объектов.
///
/// Суммарное время опроса не превышает [`KAFKA_TIMEOUT`] секунд; для клиента задаётся
/// `socket.timeout.ms` = `KAFKA_TIMEOUT * 1000` и `socket.connection.setup.timeout.ms` из [`KAFKA_CONNECT_TIMEOUT`].
/// Сообщения без payload (tombstone) пропускаются.
pub fn read_log_entries_from_kafka() -> Result<Vec<LogEntry>, ReadLogEntriesError> {
    // Гарантируем, что `.env` загружен до чтения переменных окружения.
    env_work::ensure_dotenv_loaded();

    // Таймауты берём из статиков (секунды) и конвертируем в миллисекунды для параметров клиента.
    let timeout_secs = *KAFKA_TIMEOUT;
    let connect_timeout_ms = (*KAFKA_CONNECT_TIMEOUT).saturating_mul(1000).max(1);
    let socket_timeout_ms = timeout_secs.saturating_mul(1000).max(1);

    // Адрес брокеров и имя топика задаются через `env_work` (KAFKA_BROKERS / KAFKA_TOPIC).
    let brokers = env_work::KAFKA_BROKERS.as_str();
    let topic = env_work::KAFKA_TOPIC.as_str();

    // Уникальная consumer group на каждый запуск функции: читаем как «одноразовый» consumer.
    let group_id = format!(
        "from-kafka-iceberg-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );

    // Создаём consumer:
    // - `auto.offset.reset=earliest` — если нет коммитов в группе, читаем с начала
    // - `enable.auto.commit=false` — чтение без фиксации offsets (идемпотентность/контроль выше по потоку)
    let consumer: BaseConsumer = ClientConfig::new()
        .set("bootstrap.servers", brokers)
        .set("group.id", &group_id)
        .set("auto.offset.reset", "earliest")
        .set("enable.auto.commit", "false")
        .set("socket.timeout.ms", &socket_timeout_ms.to_string())
        .set(
            "socket.connection.setup.timeout.ms",
            &connect_timeout_ms.to_string(),
        )
        .create()?;

    // Подписываемся на один топик из конфигурации.
    consumer.subscribe(&[topic])?;

    // Ограничиваем суммарное время чтения сверху KAFKA_TIMEOUT (deadline),
    // при этом опрашиваем небольшими порциями, чтобы не «зависать» одним poll на весь таймаут.
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let poll_slice = Duration::from_millis(200);

    let mut out = Vec::new();

    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let wait = poll_slice.min(remaining);
        if wait.is_zero() {
            break;
        }

        match consumer.poll(wait) {
            None => {}
            Some(Ok(msg)) => {
                // Tombstone / сообщение без payload пропускаем.
                let Some(payload) = msg.payload() else {
                    continue;
                };
                // Поддерживаем два формата: JSON-массив LogEntry или один LogEntry.
                let mut entries = parse_json_payload_to_entries(payload)?;
                out.append(&mut entries);
            }
            Some(Err(e)) => return Err(ReadLogEntriesError::Kafka(e)),
        }
    }

    Ok(out)
}

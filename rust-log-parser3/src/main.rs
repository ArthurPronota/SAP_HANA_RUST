//! # rust-log-parser
//!
//! Консольное приложение для **разбора** текстовых лог-файлов в структуры [`LogEntry`], при
//! необходимости — **отправки** массива записей в **Apache Kafka** в виде одной JSON-строки и
//! **ведения служебного журнала** в каталоге `parser_logs/` (неблокирующая запись через `tracing`).
//!
//! ## Запуск
//!
//! ```text
//! cargo run -- --log_file путь/к/файлу.log
//! ```
//!
//! Параметры окружения и `.env` описаны в файле **README.md** в корне проекта. Список брокеров Kafka и топик задаются в
//! модуле [`env_work`]; таймауты клиента — в [`kafka`].
//!
//! ## Модули
//!
//! | Модуль | Назначение |
//! |--------|------------|
//! | [`env_work`] | Загрузка `.env`, переменные `KAFKA_BROKERS`, `KAFKA_TOPIC` |
//! | [`kafka`] | `KAFKA_*_TIMEOUT`, [`kafka::send_log_entries_to_kafka`] |
//! | [`my_log`] | Инициализация `tracing`, файлы в `parser_logs/` |
//!
//! ## Формат строки лога
//!
//! Шаблон задан регулярным выражением в [`parse_log_lines`]. Он рассчитан на строки с префиксом
//! вида `[…]{…}[…]`, меткой времени, уровнем, компонентом и сообщением; для других форматов (в т.ч.
//! сырых логов SAP HANA) правило может потребовать изменения.

use clap::Parser;
use regex::Regex;
use serde::Serialize;
use std::fs;
use std::path::PathBuf;

mod env_work;
mod kafka;
mod my_log;

pub use env_work::{KAFKA_BROKERS, KAFKA_TOPIC};
pub use kafka::{
    kafka_brokers_and_topic_mix, send_log_entries_to_kafka, KAFKA_CONNECT_TIMEOUT, KAFKA_TIMEOUT,
};

/// Одна запись из лога после разбора.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: char,
    pub component: String,
    pub message: String,
}

#[derive(Parser, Debug)]
#[command(about = "Разбор лог-файла в структуры LogEntry")]
struct Args {
    /// Путь к файлу лога
    #[arg(long = "log_file", required = true)]
    log_file: PathBuf,
}

/// Читает файл и возвращает вектор записей для строк заданного формата.
pub fn parse_log_file(path: &std::path::Path) -> Result<Vec<LogEntry>, std::io::Error> {
    let content = fs::read_to_string(path)?;
    Ok(parse_log_lines(&content))
}

/// Разбивает переданный текст на строки и для каждой строки пытается извлечь поля [`LogEntry`]
/// по встроенному регулярному выражению.
///
/// Строки, которые **не** совпали с шаблоном, пропускаются (в результат не попадают).
///
/// Шаблон использует флаги `(?m)` (якоря `^`/`$` привязаны к строкам) и `(?x)` (в шаблоне игнорируются
/// пробелы и переводы строк, допускаются комментарии `# …`). Захваты групп:
///
/// 1. дата и время;
/// 2. один символ уровня логирования;
/// 3. имя компонента;
/// 4. текст сообщения до конца строки.
///
/// Между датой и временем в шаблоне используется `\s+`, а не литеральный пробел — иначе в режиме `(?x)`
/// пробел был бы проигнорирован.
pub fn parse_log_lines(content: &str) -> Vec<LogEntry> {
    // (?m) — ^/$ по строкам; (?x) — пробелы и переводы строк в шаблоне не матчатся, можно писать #-комментарии.
    let re = Regex::new(
        r"(?mx)
^                                       # начало строки
\[[^\]]*\]                              # префикс, напр. [43125]
\{[^}]*\}                               # блок, напр. {-1}
\[[^\]]*\]                              # напр. [-1/-1]
\s+
(\d{4}-\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}\.\d+)  # 1: дата и время; \s+ — пробел между датой и временем (в (?x) нельзя «голый» пробел)
\s+
([a-zA-Z])                              # 2: уровень лога
\s+
(\S+)                                   # 3: компонент
\s+
\S+                                     # имя файла
\([^)]*\)                               # строка в исходнике, напр. (00123)
\s*:\s*
(.*)$                                   # 4: сообщение
",
    )
    .expect("valid regex");

    let mut out = Vec::new();
    for line in content.lines() {
        if let Some(caps) = re.captures(line) {
            let timestamp = caps
                                        .get(1)
                                        .map(|m| m.as_str().to_string())
                                        .unwrap_or_default()
                                        ;
            let level = caps
                .get(2)
                .and_then(|m| m.as_str().chars().next())
                .unwrap_or('?') ;
            let component = caps.get(3).map(|m| m.as_str().to_string()).unwrap_or_default();
            let message = caps.get(4).map(|m| m.as_str().to_string()).unwrap_or_default();
            out.push(LogEntry {
                timestamp,
                level,
                component,
                message,
            });
        }
    }
    out
}

/// Точка входа: инициализация логирования в `parser_logs/`, разбор CLI (`--log_file`), проверка
/// существования файла, загрузка настроек Kafka из `.env` ([`kafka_brokers_and_topic_mix`]),
/// чтение и разбор лога ([`parse_log_file`]). При ненулевом числе записей — отправка в Kafka
/// ([`send_log_entries_to_kafka`]). Ошибки чтения файла и отправки в Kafka логируются и ведут к
/// завершению процесса с кодом 1.
#[tokio::main]
async fn main() {
    let _log_guard = my_log::init();

    let args = Args::parse();
    tracing::info!(log_file = %args.log_file.display(), "старт разбора");

    if !args.log_file.exists() {
        tracing::error!(path = %args.log_file.display(), "файл лога не найден");
        eprintln!(
            "Ошибка: файл «{}» не существует.",
            args.log_file.display()
        );
        std::process::exit(1);
    }

    let (brokers, topic, ..) = kafka_brokers_and_topic_mix();

    match parse_log_file(&args.log_file) {
        Ok(entries) => {
            tracing::info!(count = entries.len(), "разбор завершён");
            // println!("Разобрано записей: {}", entries.len());
            if entries.len() > 0 {
                match send_log_entries_to_kafka(&brokers, &topic, &entries).await {
                    Ok(()) => {
                        tracing::info!("записи отправлены в Kafka");
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "ошибка отправки в Kafka");
                        eprintln!("Ошибка отправки в Kafka: {e}");
                        std::process::exit(1);
                    }
                }
            }

            // for e in &entries {
            //     println!(
            //         "{:?} | {} | {} | {}",
            //         e.timestamp, e.level, e.component, e.message
            //     );
            // }
        }
        Err(e) => {
            tracing::error!(error = %e, "ошибка чтения файла");
            eprintln!("Ошибка чтения файла: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sample_line() {
        let line = r"[43125]{-1}[-1/-1] 2024-05-20 10:15:30.123 i GlobalRowStore    GlobalRowStore.cpp(00123) : Global RowStore Runtime Started.";
        let v = parse_log_lines(line);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].timestamp, "2024-05-20 10:15:30.123");
        assert_eq!(v[0].level, 'i');
        assert_eq!(v[0].component, "GlobalRowStore");
        assert_eq!(v[0].message, "Global RowStore Runtime Started.");
    }

    /// Отправка в Kafka с `KAFKA_BROKERS` / `KAFKA_TOPIC` из `.env`; нужен запущенный брокер.
    /// Запуск: `cargo test sends_parsed_line_to_kafka -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn sends_parsed_line_to_kafka() {
        let line = r"[43125]{-1}[-1/-1] 2024-05-20 10:15:30.123 i GlobalRowStore    GlobalRowStore.cpp(00123) : Global RowStore Runtime Started.";
        let v = parse_log_lines(line);
        send_log_entries_to_kafka(KAFKA_BROKERS.as_str(), KAFKA_TOPIC.as_str(), &v)
            .await
            .expect("отправка в Kafka");
    }
}

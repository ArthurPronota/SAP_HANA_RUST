use regex::Regex;
use std::fs::File;
use std::io::{self, BufRead};
use anyhow::Result ;

use std::sync::Arc;

use arrow::array::StringArray;
use arrow::record_batch::RecordBatch;
use arrow::datatypes::{DataType, Field, Schema as ArrowSchema};

#[derive(Debug)]
struct HanaLogEntry {
    timestamp: String,
    level: char,
    component: String,
    message: String,
}

/// Преобразование Vec в Arrow RecordBatch
fn entries_to_arrow(entries: &[HanaLogEntry]) -> RecordBatch {
    let arrow_schema = Arc::new(ArrowSchema::new(vec![
        Field::new("timestamp", DataType::Utf8, false),
        Field::new("level", DataType::Utf8, false),
        Field::new("component", DataType::Utf8, false),
        Field::new("message", DataType::Utf8, false),
    ]));

    let timestamps = StringArray::from_iter_values(entries.iter().map(|e| &e.timestamp));
    let levels = StringArray::from_iter_values(entries.iter().map(|e| e.level.to_string()));
    let components = StringArray::from_iter_values(entries.iter().map(|e| &e.component));
    let messages = StringArray::from_iter_values(entries.iter().map(|e| &e.message));

    RecordBatch::try_new(
        arrow_schema,
        vec![
            Arc::new(timestamps),
            Arc::new(levels),
            Arc::new(components),
            Arc::new(messages),
        ],
    ).expect("Failed to create RecordBatch")
}


#[tokio::main]
async fn main() -> Result<()>
{
    let path = "logs/indexserver.trc";
    
    // Регулярное выражение для парсинга структуры HANA Trace
    // Принимает формат: {Thread}[Conn/Trans] Timestamp Level Component Source : Message
    let log_re = Regex::new(r"\{?(-?\d+)\}?\[?(-?\d+/?-?\d*)\]?\s+(?P<ts>\d{4}-\d{2}-\d{2}\s\d{2}:\d{2}:\d{2}\.\d+)\s+(?P<level>[iwe])\s+(?P<comp>\w+)\s+[^:]+:\s+(?P<msg>.*)").unwrap();

    let file = File::open(path)?;
    let reader = io::BufReader::new(file);

    println!("--- Анализ критических событий SAP HANA ---");

    let mut logs = vec![] ;

    for line in reader.lines() {
        let line = line?;
        if let Some(caps) = log_re.captures(&line) {
            let level = caps["level"].chars().next().unwrap();
            
            // Фильтруем только Ошибки (e) и Предупреждения (w)
            if level == 'e' || level == 'w' {
                let entry = HanaLogEntry {
                    timestamp: caps["ts"].to_string(),
                    level,
                    component: caps["comp"].to_string(),
                    message: caps["msg"].to_string(),
                };

                // Вывод с элементами "интеллектуального" анализа
                match entry.level {
                    'e' => print!("[КРИТИЧНО] "),
                    'w' => print!("[ВНИМАНИЕ] "),
                    _ => unreachable!(),
                }
                
                println!("{} | {}: {}", entry.timestamp, entry.component, entry.message);

                // Пример реакции на конкретную проблему
                if entry.message.contains("OUT OF MEMORY") {
                    println!("   Проверьте лимиты выделения памяти (GAL) и тяжелые запросы.");
                }

                logs.push(entry);
            }
        }
    }

    // --- ШАГ 3: Конвертация и "запись" ---
    let batch = entries_to_arrow(&logs);
    
    // для примера выведем RecordBatch, готовый к отправке в Iceberg
    println!("Arrow RecordBatch готов к записи в Iceberg:");
    println!("{:?}", batch);


    Ok(())
}

use std::fs::File;
use std::io::{self, BufRead};

#[derive(Default, Debug)]
struct DumpSummary {
    timestamp: String,
    memory_used_gb: f64,
    memory_limit_gb: f64,
    heavy_queries: Vec<String>,
}

fn main() -> io::Result<()> {
    let path = "indexserver_service.rtedump"; // Путь к дампу
    let file = File::open(path)?;
    let reader = io::BufReader::new(file);

    let mut summary = DumpSummary::default();
    let mut current_section = String::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();

        // 1. Определяем текущую секцию
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current_section = trimmed.to_string();
            continue;
        }

        // 2. Парсим данные в зависимости от секции
        match current_section.as_str() {
            "[SYSTEM_INFO]" => {
                if trimmed.starts_with("Timestamp:") {
                    summary.timestamp = trimmed.replace("Timestamp:", "").trim().to_string();
                }
            }
            "[MEMORY_STATUS]" => {
                if trimmed.starts_with("Used:") {
                    summary.memory_used_gb = parse_gb(trimmed);
                } else if trimmed.starts_with("Limit:") {
                    summary.memory_limit_gb = parse_gb(trimmed);
                }
            }
            "[THREADS]" => {
                // Ищем строки с SQL запросами в активных потоках
                if trimmed.starts_with("Thread") && trimmed.contains("SQL:") {
                    if let Some(sql) = trimmed.split("SQL:").last() {
                        summary.heavy_queries.push(sql.trim().to_string());
                    }
                }
            }
            _ => {}
        }
    }

    // Диагностический вывод
    println!("--- Результат анализа SAP HANA Dump ---");
    println!("Время дампа: {}", summary.timestamp);
    println!("Память: {} / {} GB", summary.memory_used_gb, summary.memory_limit_gb);
    
    if summary.memory_used_gb >= summary.memory_limit_gb * 0.95 {
        println!("🛑 КРИТИЧЕСКИЙ СТАТУС: Обнаружена нехватка памяти (OOM)!");
    }

    if !summary.heavy_queries.is_empty() {
        println!("🔍 Тяжелые запросы в момент сбоя:");
        for sql in summary.heavy_queries {
            println!("   - {}", sql);
        }
    }

    Ok(())
}

// Хелпер для извлечения чисел из строк типа "Used: 62.5 GB"
fn parse_gb(line: &str) -> f64 {
    line.split_whitespace()
        .find_map(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0)
}
use regex::Regex;
use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;

#[derive(Debug)]
struct HanaLogEntry {
    timestamp: String,
    level: char,
    component: String,
    message: String,
}

fn main() -> io::Result<()> {
    let path = "logs/indexserver.trc";
    
    // Регулярное выражение для парсинга структуры HANA Trace
    // Принимает формат: {Thread}[Conn/Trans] Timestamp Level Component Source : Message
    let log_re = Regex::new(r"\{?(-?\d+)\}?\[?(-?\d+/?-?\d*)\]?\s+(?P<ts>\d{4}-\d{2}-\d{2}\s\d{2}:\d{2}:\d{2}\.\d+)\s+(?P<level>[iwe])\s+(?P<comp>\w+)\s+[^:]+:\s+(?P<msg>.*)").unwrap();

    let file = File::open(path)?;
    let reader = io::BufReader::new(file);

    println!("--- Анализ критических событий SAP HANA ---");

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
                    'e' => print!("🔴 [КРИТИЧНО] "),
                    'w' => print!("🟡 [ВНИМАНИЕ] "),
                    _ => unreachable!(),
                }
                
                println!("{} | {}: {}", entry.timestamp, entry.component, entry.message);

                // Пример реакции на конкретную проблему
                if entry.message.contains("OUT OF MEMORY") {
                    println!("   👉 СОВЕТ: Проверьте лимиты выделения памяти (GAL) и тяжелые запросы.");
                }
            }
        }
    }

    Ok(())
}

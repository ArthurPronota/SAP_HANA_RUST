//! Запись [`crate::LogEntry`] в **Apache Iceberg** (крейт [`iceberg`](https://crates.io/crates/iceberg) — реализация [iceberg-rust](https://github.com/apache/iceberg-rust)).
//!
//! Используется [`MemoryCatalog`](::iceberg::memory::MemoryCatalog) и каталог warehouse на диске.
//!
//! # Переменные окружения
//!
//! - Корень warehouse задаётся в [`crate::env_work`] как статик [`ICEBERG_WAREHOUSE`](crate::env_work::ICEBERG_WAREHOUSE) (переменная `ICEBERG_WAREHOUSE` в `.env`). Значение передаётся в memory-каталог как [`MEMORY_CATALOG_WAREHOUSE`]: абсолютный путь к каталогу на локальной ФС.
//!
//! Примеры значения:
//! - Windows: `C:\Data\iceberg-warehouse` или `C:/Data/iceberg-warehouse`
//! - Linux/macOS: `/var/data/iceberg-warehouse`
//!
//! Каталог может не существовать заранее — при необходимости его можно создать вручную; важно, что путь доступен процессу на запись.
//!
//! Таблица **`logging.log_entries`** создаётся при отсутствии. Поля Iceberg: `timestamp`, `level`, `component`, `message` (строки; уровень — один символ как строка).
//!
//! # Функция [`send_log_entries_to_iceberg`]
//!
//! Асинхронно пишет переданные строки в Iceberg (Parquet + fast append). Пустой слайс — успех без обращения к каталогу и без чтения [`ICEBERG_WAREHOUSE`](crate::env_work::ICEBERG_WAREHOUSE). Успех: `Ok(())`; ошибка: [`::iceberg::Error`].

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::{ArrayRef, RecordBatch, StringArray};
use ::iceberg::arrow::schema_to_arrow_schema;
use ::iceberg::memory::{MemoryCatalogBuilder, MEMORY_CATALOG_WAREHOUSE};
use ::iceberg::spec::{DataFileFormat, NestedField, PrimitiveType, Schema, Type};
use ::iceberg::transaction::{ApplyTransactionAction, Transaction};
use ::iceberg::writer::base_writer::data_file_writer::DataFileWriterBuilder;
use ::iceberg::writer::file_writer::location_generator::{
    DefaultFileNameGenerator, DefaultLocationGenerator,
};
use ::iceberg::writer::file_writer::rolling_writer::RollingFileWriterBuilder;
use ::iceberg::writer::file_writer::ParquetWriterBuilder;
use ::iceberg::writer::{IcebergWriter, IcebergWriterBuilder};
use ::iceberg::{Catalog, CatalogBuilder, TableCreation, TableIdent};
use parquet::file::properties::WriterProperties;

use crate::env_work;
use crate::LogEntry;

/// Схема Iceberg для таблицы логов, согласованная с [`LogEntry`].
///
/// Идентификаторы полей (1–4) фиксированы: их нельзя менять после создания таблицы без миграции.
/// Все колонки обязательные строки; уровень лога в Rust — `char`, в таблице хранится как строка из одного символа.
fn log_entry_iceberg_schema() -> Schema {
    Schema::builder()
        .with_fields(vec![
            // Метка времени строки лога (как в исходном тексте).
            NestedField::required(1, "timestamp", Type::Primitive(PrimitiveType::String)).into(),
            // Уровень (i/e/w и т.д.) одним символом в виде UTF-8 строки.
            NestedField::required(2, "level", Type::Primitive(PrimitiveType::String)).into(),
            // Имя компонента / подсистемы.
            NestedField::required(3, "component", Type::Primitive(PrimitiveType::String)).into(),
            // Текст сообщения до конца строки лога.
            NestedField::required(4, "message", Type::Primitive(PrimitiveType::String)).into(),
        ])
        .build()
        .expect("log_entry_iceberg_schema")
}

fn log_entries_table_ident() -> ::iceberg::Result<TableIdent> {
    TableIdent::from_strs(["logging", "log_entries"])
}

async fn load_or_create_table<C: Catalog>(
    catalog: &C,
    ident: &TableIdent,
) -> ::iceberg::Result<::iceberg::table::Table> {
    if catalog.table_exists(ident).await? {
        catalog.load_table(ident).await
    } else {
        if !catalog.namespace_exists(&ident.namespace).await? {
            catalog
                .create_namespace(&ident.namespace, HashMap::new())
                .await?;
        }
        catalog
            .create_table(
                &ident.namespace,
                TableCreation::builder()
                    .name(ident.name().to_string())
                    .schema(log_entry_iceberg_schema())
                    .build(),
            )
            .await
    }
}

/// Записывает [`LogEntry`] в Iceberg: Parquet data file и fast-append снимка таблицы.
///
/// Пустой слайс завершается успехом без обращения к каталогу.
///
/// # Пример `ICEBERG_WAREHOUSE` в `.env`
///
/// ```text
/// ICEBERG_WAREHOUSE=C:/Data/iceberg-warehouse
/// ```
///
/// # Пример вызова
///
/// ```ignore
/// # async fn demo(entries: Vec<crate::LogEntry>) -> ::iceberg::Result<()> {
/// crate::iceberg::send_log_entries_to_iceberg(&entries).await?;
/// # Ok(())
/// # }
/// ```
pub async fn send_log_entries_to_iceberg(entries: &[LogEntry]) -> ::iceberg::Result<()> {
    // Подхватить .env до чтения ICEBERG_WAREHOUSE и остальных переменных.
    env_work::ensure_dotenv_loaded();
    // Нечего писать — не трогаем каталог и не требуем warehouse.
    if entries.is_empty() {
        return Ok(());
    }

    // Путь warehouse из env_work (тот же ключ, что в .env: ICEBERG_WAREHOUSE).
    let warehouse = env_work::ICEBERG_WAREHOUSE.clone();

    // In-memory каталог Iceberg; warehouse — корень на диске для метаданных и data-файлов.
    let catalog = MemoryCatalogBuilder::default()
        .load(
            "memory",
            HashMap::from([(MEMORY_CATALOG_WAREHOUSE.to_string(), warehouse)]),
        )
        .await?;

    // Таблица logging.log_entries: загрузить или создать вместе с namespace.
    let ident = log_entries_table_ident()?;
    let table = load_or_create_table(&catalog, &ident).await?;

    // Схема таблицы в формате Iceberg и совместимая Arrow-схема (с field id для Parquet).
    let iceberg_schema = table.current_schema_ref();
    let arrow_schema = Arc::new(schema_to_arrow_schema(iceberg_schema.as_ref())?);

    // Колонки Arrow из полей LogEntry (порядок совпадает со схемой таблицы).
    let timestamp = StringArray::from_iter_values(entries.iter().map(|e| e.timestamp.as_str()));
    let level = StringArray::from_iter_values(entries.iter().map(|e| e.level.to_string()));
    let component = StringArray::from_iter_values(entries.iter().map(|e| e.component.as_str()));
    let message = StringArray::from_iter_values(entries.iter().map(|e| e.message.as_str()));

    let batch = RecordBatch::try_new(
        arrow_schema.clone(),
        vec![
            Arc::new(timestamp) as ArrayRef,
            Arc::new(level) as ArrayRef,
            Arc::new(component) as ArrayRef,
            Arc::new(message) as ArrayRef,
        ],
    )
    .map_err(|e| {
        ::iceberg::Error::new(
            ::iceberg::ErrorKind::DataInvalid,
            format!("RecordBatch: {e}"),
        )
    })?;

    // Куда и под каким именем писать Parquet относительно location таблицы.
    let location_generator = DefaultLocationGenerator::new(table.metadata().clone())?;
    let file_name_generator = DefaultFileNameGenerator::new(
        "log_entries".to_string(),
        None,
        DataFileFormat::Parquet,
    );

    // Цепочка: Parquet → rolling file → logical data file writer Iceberg.
    let parquet_writer_builder =
        ParquetWriterBuilder::new(WriterProperties::default(), iceberg_schema);

    let rolling = RollingFileWriterBuilder::new_with_default_file_size(
        parquet_writer_builder,
        table.file_io().clone(),
        location_generator,
        file_name_generator,
    );

    let data_file_writer_builder = DataFileWriterBuilder::new(rolling);
    let mut data_file_writer = data_file_writer_builder.build(None).await?;

    // Записать батч и закрыть writer — получить список DataFile для метаданных.
    data_file_writer.write(batch).await?;
    let data_files = data_file_writer.close().await?;

    // Зафиксировать новые data files в таблице (fast append нового снимка).
    let tx = Transaction::new(&table);
    let tx = tx.fast_append().add_data_files(data_files).apply(tx)?;
    let _ = tx.commit(&catalog).await?;
    Ok(())
}

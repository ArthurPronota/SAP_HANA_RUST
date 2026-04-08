use hdbconnect_async::{Connection, HdbResult};

#[tokio::main]
async fn main() -> HdbResult<()> {
    // Строка подключения: "hdbsql://USER:PASSWORD@HOST:PORT"
    let connection = Connection::new(
        "hdbsql://GE310236:Obac1zv!t2oA9y1@hana-cloud-instance:443"
    ).await?;

    // Создание таблицы
    connection.multiple_statements(vec![
        "CREATE TABLE users (id INT PRIMARY KEY, name NVARCHAR(100))"
    ]).await?;

    // Вставка данных через подготовленный запрос
    let mut insert_stmt = connection.prepare(
        "INSERT INTO users (id, name) VALUES (?, ?)"
    ).await?;
    insert_stmt.add_batch(&(1, "Alice"))?;
    insert_stmt.add_batch(&(2, "Bob"))?;
    insert_stmt.execute_batch().await?;

    // Запрос данных
    let users: Vec<(i32, String)> = connection
        .query("SELECT * FROM users")
        .await?
        .try_into()
        .await?;

    for (id, name) in users {
        println!("{}: {}", id, name);
    }

    Ok(())
}
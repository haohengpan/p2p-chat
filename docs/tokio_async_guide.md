# Tokio 异步编程指南

本文档专门针对有一定 Rust 基础但不熟悉 Tokio 和异步编程的开发者，帮助你快速上手这个项目的异步代码。

## 1. 什么是异步编程？

### 1.1 同步 vs 异步

**同步编程**：
```rust
// 顺序执行，每步都要等待完成
let result1 = blocking_function();
let result2 = blocking_function();
let result3 = blocking_function();
```

**异步编程**：
```rust
// 异步执行，在等待时可以做其他事情
let result1 = async_function1().await;
let result2 = async_function2().await;
let result3 = async_function3().await;
```

### 1.2 异步编程的优势

对于网络应用：
- **并发处理**：可以同时处理多个连接
- **资源利用**：等待网络 I/O 时不阻塞线程
- **性能提升**：更高的吞吐量和响应速度

## 2. Tokio 运行时

### 2.1 Tokio 是什么？

Tokio 是 Rust 的异步运行时，提供了：
- 异步任务调度
- 异步 I/O 操作（TCP、UDP、文件等）
- 定时器和超时
- 同步原语

### 2.2 创建运行时

```rust
#[tokio::main]  // 宏，自动创建运行时
async fn main() -> Result<()> {
    // 异步代码
    Ok(())
}
```

等价于：
```rust
fn main() {
    // 创建运行时
    let runtime = tokio::runtime::Runtime::new().unwrap();

    // 在运行时中执行异步主函数
    runtime.block_on(async {
        // 异步代码
    });
}
```

### 2.3 使用 tokio::select!

`tokio::select!` 是一个强大的工具，用于同时等待多个异步操作：

```rust
tokio::select! {
    // 等待服务器任务完成
    result = server(port, host, max_clients) => {
        if let Err(e) = result {
            error!("Server error: {}", e);
        }
    }

    // 等待 Ctrl+C 信号
    _ = tokio::signal::ctrl_c() => {
        info!("Received shutdown signal");
    }
}
```

常见模式：
```rust
// 带超时的等待
tokio::select! {
    result = some_async_operation() => {
        // 操作完成
    }
    _ = tokio::time::sleep(Duration::from_secs(5)) => {
        // 超时
    }
}

// 处理多个事件源
tokio::select! {
    msg = receiver.recv() => {
        // 接收到消息
    }
    _ = socket.readable() => {
        // socket 可读
    }
}
```

## 3. 异步 I/O 操作

### 3.1 TCP 服务器

```rust
async fn server(port: u16) -> Result<()> {
    // 创建 TCP 监听器（异步）
    let listener = tokio::net::TcpListener::bind(&format!("0.0.0.0:{}", port)).await?;

    loop {
        // 异步接受连接（非阻塞）
        let (socket, addr) = listener.accept().await?;

        // 每个连接 spawn 一个独立任务
        tokio::spawn(async move {
            if let Err(e) = handle_connection(socket, addr).await {
                error!("Error handling connection: {}", e);
            }
        });
    }
}
```

### 3.2 TCP 客户端

```rust
async fn client(address: SocketAddr) -> Result<()> {
    // 异步连接到服务器
    let socket = tokio::net::TcpStream::connect(address).await?;

    // 分离读写流
    let (mut reader, mut writer) = tokio::io::split(socket);

    // 并发读写
    tokio::try_join!(
        async {
            // 读取任务
            loop {
                let mut buffer = vec![0u8; 1024];
                let n = reader.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                println!("Received: {}", String::from_utf8_lossy(&buffer[..n]));
            }
            Ok::<(), std::io::Error>(())
        },
        async {
            // 写入任务
            loop {
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                writer.write_all(input.as_bytes()).await?;
            }
            Ok::<(), std::io::Error>(())
        }
    )?;

    Ok(())
}
```

### 3.3 文件 I/O

```rust
// 异步读取文件
async fn read_file_async(path: &str) -> Result<String> {
    let file = tokio::fs::File::open(path).await?;
    let mut reader = tokio::io::BufReader::new(file);
    let mut content = String::new();
    reader.read_to_string(&mut content).await?;
    Ok(content)
}

// 异步写入文件
async fn write_file_async(path: &str, content: &str) -> Result<()> {
    let file = tokio::fs::File::create(path).await?;
    let mut writer = tokio::io::BufWriter::new(file);
    writer.write_all(content.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}
```

## 4. 任务和并发

### 4.1 创建任务

```rust
// 使用 spawn 创建任务（返回 JoinHandle）
let handle = tokio::spawn(async {
    // 异步代码
    "Task result"
});

// 等待任务完成
let result = handle.await?;
```

### 4.2 任务通信

使用 mpsc（多生产者单消费者）通道：

```rust
// 创建通道
let (tx, mut rx) = tokio::sync::mpsc::channel(100);

// 在任务中发送消息
tokio::spawn(async move {
    for i in 0..10 {
        tx.send(format!("Message {}", i)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
});

// 接收消息
while let Some(message) = rx.recv().await {
    println!("Received: {}", message);
}
```

### 4.3 共享状态

使用 Arc + Mutex：

```rust
use std::sync::{Arc, Mutex};

// 创建共享状态
let counter = Arc::new(Mutex::new(0));

// 克隆 Arc 并在不同任务中使用
let counter_clone = Arc::clone(&counter);

tokio::spawn(async move {
    let mut num = counter_clone.lock().unwrap();
    *num += 1;
});

// 等待并读取结果
tokio::time::sleep(Duration::from_millis(100)).await;
let final_count = *counter.lock().unwrap();
println!("Final count: {}", final_count);
```

### 4.4 原子操作

对于不需要加锁的简单操作：

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

let counter = Arc::new(AtomicUsize::new(0));

let counter_clone = Arc::clone(&counter);

tokio::spawn(async move {
    // 原子递增
    counter_clone.fetch_add(1, Ordering::SeqCst);
});

let value = counter.load(Ordering::SeqCst);
println!("Counter value: {}", value);
```

## 5. 定时器和超时

### 5.1 延迟执行

```rust
// 1秒后执行
tokio::time::sleep(Duration::from_secs(1)).await;

// 延迟执行闭包
tokio::time::timeout(Duration::from_secs(5), async {
    // 异步操作
    "result"
}).await?;
```

### 5.2 周期性任务

```rust
let mut interval = tokio::time::interval(Duration::from_secs(1));

loop {
    interval.tick().await;
    println!("每秒执行一次");
}
```

### 5.3 超时控制

```rust
tokio::select! {
    result = long_running_operation() => {
        println!("操作完成");
    }
    _ = tokio::time::sleep(Duration::from_secs(5)) => {
        println!("操作超时");
    }
}
```

## 6. 流处理

### 6.1 异步迭代器

```rust
// 逐行读取文件
let file = tokio::fs::File::open("data.txt").await?;
let mut reader = BufReader::new(file);
let mut line = String::new();

while reader.read_line(&mut line).await? != 0 {
    println!("Line: {}", line.trim());
    line.clear();
}
```

### 6.2 消息流

```rust
let mut stream = futures::stream::iter(vec![1, 2, 3, 4, 5]);

while let Some(item) = stream.next().await {
    println!("Item: {}", item);
}
```

## 7. 错误处理

### 7.1 使用 Result 和 ?

```rust
async fn process_data() -> Result<String, anyhow::Error> {
    // 读取文件（如果失败，自动返回错误）
    let content = tokio::fs::read_to_string("data.txt").await?;

    // 解析 JSON（如果失败，自动返回错误）
    let data: serde_json::Value = serde_json::from_str(&content)?;

    // 返回结果
    Ok(data.to_string())
}
```

### 7.2 错误转换

```rust
use thiserror::Error;

#[derive(Error, Debug)]
enum MyError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

async fn do_something() -> Result<(), MyError> {
    let content = tokio::fs::read_to_string("file.txt").await?; // 自动转换为 MyError
    Ok(())
}
```

## 8. 实际应用示例

### 8.1 HTTP 客户端

```rust
// 发起异步 HTTP 请求
async fn fetch_url(url: &str) -> Result<String> {
    let response = reqwest::get(url).await?;
    let body = response.text().await?;
    Ok(body)
}
```

### 8.2 并行处理

```rust
async fn process_multiple_items(items: Vec<String>) -> Vec<String> {
    // 使用 futures::future::join_all 并发执行多个任务
    let futures: Vec<_> = items.into_iter().map(|item| {
        async move {
            // 处理单个项目
            process_item(item).await
        }
    }).collect();

    // 等待所有任务完成
    let results = futures::future::join_all(futures).await;

    // 收集结果
    results.into_iter().filter_map(|r| r.ok()).collect()
}
```

### 8.3 生产者消费者模式

```rust
async fn producer_consumer() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);

    // 启动生产者任务
    let producer = tokio::spawn(async move {
        for i in 0..10 {
            tx.send(format!("Item {}", i)).await.unwrap();
        }
    });

    // 启动消费者任务
    let consumer = tokio::spawn(async move {
        while let Some(item) = rx.recv().await {
            println!("Consumed: {}", item);
        }
    });

    // 等待所有任务完成
    let _ = tokio::join!(producer, consumer);
}
```

## 9. 最佳实践

### 9.1 性能优化

1. **避免阻塞操作**：
   ```rust
   // 错误：阻塞操作在异步代码中
   let data = std::fs::read_to_string("file.txt").unwrap();

   // 正确：使用异步版本
   let data = tokio::fs::read_to_string("file.txt").await?;
   ```

2. **合理使用缓冲**：
   ```rust
   // 使用 BufReader/BufWriter 提高性能
   let file = tokio::fs::File::open("file.txt").await?;
   let mut reader = tokio::io::BufReader::new(file);
   ```

3. **控制任务数量**：
   ```rust
   // 使用 bounded channel 控制并发任务数
   let (tx, rx) = tokio::sync::mpsc::channel(10);
   ```

### 9.2 资源管理

1. **及时释放资源**：
   ```rust
   // 使用 try-finally 或 drop 及时释放
   async fn handle_connection(socket: TcpStream) -> Result<()> {
       let (mut reader, mut writer) = tokio::io::split(socket);

       // 处理连接
       // ...

       Ok(())
   } // socket 自动关闭
   ```

2. **避免内存泄漏**：
   ```rust
   // 确保通道被正确关闭
   let (tx, mut rx) = tokio::sync::mpsc::channel(100);
   drop(tx); // 显式关闭生产者端
   ```

### 9.3 调试技巧

1. **使用 tracing**：
   ```rust
   use tracing::{info, error, debug};

   async fn debug_function() {
       info!("Function started");
       debug!("Debug info: {}", some_value);
       // ...
       error!("Function failed: {}", error);
   }
   ```

2. **添加超时**：
   ```rust
   // 为长时间运行的操作添加超时
   tokio::time::timeout(Duration::from_secs(5), some_operation).await?;
   ```

## 10. 总结

Tokio 是 Rust 异步编程的核心组件，掌握以下概念就能编写高性能的异步应用：

- **异步函数和 await**：编写异步代码的基础
- **任务和并发**：使用 spawn 创建并发任务
- **异步 I/O**：使用异步版本的 I/O 操作
- **tokio::select!**：同时等待多个操作
- **通信和同步**：使用通道、Arc、原子操作等
- **错误处理**：使用 Result 和 ? 操作符

通过练习这些概念，你将能够编写出高效、可靠的网络应用。
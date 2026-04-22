# CLAUDE.md

本文件为 Claude Code (claude.ai/code) 在本仓库中工作时提供指导。

## 构建与检查命令

```bash
cargo check          # 仅类型检查，不生成二进制
cargo build          # 开发构建
cargo build --release # 发布构建
cargo run --release   # 运行（会打开 TUI，需要终端环境）
```

目前没有测试套件。项目未配置 linter，可用 `cargo clippy` 做静态检查。

## 架构

P2P 聊天应用：基于 mio 的同步网络 I/O + tokio 驱动 TUI 事件循环 + ratatui 终端界面。需要支持多UI摸扩展（web等）

```
ui.rs ──► NodeCommand ──► App::run()（阻塞线程）
ui.rs ◄── AppEvent ◄───── App::run()

App 拥有 Message，Message 拥有 Network
App 拥有 node_list (HashMap<String, Node>)
```

### 各层职责

- **network.rs** — 基于 `mio::Poll` 的原始 TCP I/O。管理 `TcpListener` + 按 Token 索引的 `TcpStream` 连接。返回 `NetEvent`（Connected/Data/Disconnected）。内置 `Waker` 支持跨线程唤醒 poll。
- **message.rs** — 拥有 `Network` + 每个连接的接收缓冲区。使用 4 字节小端长度前缀 + bincode 做帧编解码。对外暴露 `Packet`（线路协议枚举）和 `NetMsg`（解码后的事件）。提供 `read()/send()/connect()/close()` 接口。
- **node.rs** — `Node` 结构体：对端身份信息（node_id、name、addr、token）+ 聊天记录（`VecDeque<ChatEntry>`，上限 32 条）。纯数据，无 I/O 依赖。
- **app.rs** — `App` 结构体：拥有 `Message` + `node_list`。`run()` 是主阻塞循环（轮询网络 + 消费命令）。将 `NetMsg` 分发到各处理方法，通过 tokio mpsc 向 UI 发送 `AppEvent`。
- **command.rs** — 通过 `Context` 处理 `NodeCommand` 各变体。包含连接/发送/广播/列表等操作。
- **context.rs** — `Context<'a>`：从 App 借用多个字段（node_id、name、listen_addr、node_list、message），供命令处理器使用。
- **event.rs** — `AppEvent`（App→UI）和 `NodeCommand`（UI→App）枚举定义。
- **ui.rs** — 两个界面：`run_setup()` 收集 node_id/用户名，`run_tui()` 是聊天主界面。使用 crossterm EventStream + tokio::select 实现异步按键/事件处理。

### 线路协议

`Packet` 枚举变体：ConnectRequest、ConnectResponse、Chat、Disconnect、System、Keepalive。使用 bincode 序列化，帧格式为 `[4字节小端长度][载荷]`。

### 关键设计决策

- 网络层使用 **mio (epoll)**，非每连接一个异步任务。单一 poll 循环，边缘触发，循环读取直到 WouldBlock。
- `App::run()` 在专用线程中阻塞于 `mio::Poll`。UI 发送命令时通过 `mio::Waker` 唤醒。
- 系统消息使用虚拟 `Node`，`node_id = "system"`，`Token(usize::MAX)`。
- Token(0) = 监听器，Token(1) = Waker，Token(2+) = 连接。

## 当前已知问题

- `main.rs` 引用了不存在的模块（`connection`、`registry`），且使用旧的 `App` API — 需要重写以匹配当前代码。
- `network.rs` 的 `send()` 在非阻塞 socket 上使用 `write_all` — 需要实现写缓冲。
- `connect()` 立即返回（非阻塞 TCP），但调用方在握手完成前就发送数据。
- app.rs/command.rs 中多处 `println!` 会破坏 TUI 界面 — 应改为通过 `AppEvent` 输出。

## 语言

用户使用中文交流。除代码/技术术语外，请用中文回复。
代码注释使用英文注释

## 注意
 以上架构部分只是目前的实现，存在不合理的地方，可以在执行时优化架构和各层职责，甚至删除掉不需要的模块

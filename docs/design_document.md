# P2P Chat 设计文档

## 0. 模块结构

```
src/
├── main.rs        CLI 参数 + 绑定端口 + 调用 setup + 启动 TUI    (~70 行)
├── event.rs       AppEvent / NodeCommand / DisplayLine / LineKind (~70 行)
├── ui.rs          Setup TUI + 聊天 TUI (ratatui + crossterm)      (~500 行)
├── node.rs        P2PNode 状态机（结构体 + 全部方法）             (~420 行)
├── network.rs     handle_* 循环 + format_time + now_secs          (~170 行)
├── connection.rs  Connection / ConnectionMap / 帧编解码            (~80 行)
├── message.rs     P2PMessage 协议枚举 + 序列化                    (~120 行)
└── registry.rs    KnownPeer / PeerRegistry / parse_peer_arg       (~100 行)
```

### 依赖方向（严格单向，无循环）

```
main.rs
  └─► ui.rs       ─► event.rs
                  ─► network.rs   ─► event.rs
                                  ─► connection.rs ─► message.rs
                                  ─► registry.rs
       node.rs    ─► event.rs
                  ─► network.rs
                  ─► connection.rs
                  ─► registry.rs
       registry.rs  (叶子模块)
       message.rs   (叶子模块)
       event.rs     (叶子模块)
       connection.rs ─► message.rs
```

---

## 1. 项目概述

### 1.1 项目目标
设计并实现一个基于 Rust 的纯点对点聊天应用程序，支持多节点连接、实时消息传递、节点信息持久化，以及全屏 TUI 交互界面。

### 1.2 核心需求
- 异步 TCP 通信（全双工 P2P）
- 多节点并发连接
- 自定义二进制消息协议
- 节点注册表：持久化已知节点信息，支持按 node_id 重连
- **零配置启动**：无需命令行参数，通过 Setup TUI 收集用户信息
- **稳定端口策略**：固定使用 9000-9004 端口范围，`connect <ip>` 并发探测，无需知道端口；支持 `--port` 固定端口
- **全屏 TUI**：Setup 设置界面 + 聊天界面（消息区 + 输入栏 + 在线节点标题栏）
- Setup 界面和聊天启动消息同时显示本机地址与 LAN 地址
- 优雅关闭（通知对端断开）

### 1.3 技术栈
- **语言**: Rust 2021 Edition
- **异步运行时**: Tokio 1.40（full features）
- **序列化（消息协议）**: Bincode + Serde
- **序列化（注册表存储）**: serde_json
- **命令行**: Clap 4.5（derive 模式）
- **TUI**: ratatui 0.29 + crossterm 0.28（event-stream feature）
- **日志**: tracing + tracing-subscriber（输出到 `p2p-chat.log` 文件）
- **错误处理**: anyhow

---

## 2. 架构设计

### 2.1 整体架构

双通道架构：UI 层与 Node 层通过两个无界 MPSC 通道解耦。

```
┌────────────────────────────────────────────────────────┐
│                      main.rs                           │
│                                                        │
│  bind_listener(port)  ← 按序尝试 DEFAULT_PORTS 9000-9004│
│                                                        │
│  run_setup(listen_addr)  ← Setup TUI（收集用户信息）    │
│    └─ 返回 (node_id, username, Terminal)               │
│                                                        │
│  创建 (cmd_tx, cmd_rx) 和 (event_tx, event_rx)         │
│                                                        │
│  tokio::spawn → node.start(cmd_rx, listener) (后台)    │
│  run_tui(terminal, event_rx, cmd_tx)         (主任务)  │
└────────────────────────────────────────────────────────┘
         │                        │
         │ NodeCommand             │ AppEvent
         │ UnboundedSender         │ UnboundedReceiver
         ▼                        ▲
┌──────────────────────┐  ┌──────────────────────────────┐
│   P2PNode (node.rs)  │  │    TUI App (ui.rs)            │
│                      │  │                               │
│  cmd_rx 循环          │  │  event_rx 循环                │
│  spawn_accept_loop   │  │  App { messages,              │
│  connect_to_peer     │  │       current_chat,           │
│  send/broadcast      │  │       online_peers,           │
│  shutdown            │  │       scroll_offset,          │
│                      │  │       history }               │
└──────────┬───────────┘  └──────────────────────────────┘
           │
           │ event_tx (clone)
           ▼
┌──────────────────────────────────────────────────────┐
│                  Network Layer                        │
│  handle_incoming_connection                          │
│  handle_read_loop    ← emits AppEvent via event_tx   │
│  handle_write_loop                                   │
└──────────────────────────────────────────────────────┘
```

### 2.2 核心组件

#### 2.2.1 event.rs — 通信类型

```rust
enum AppEvent {
    MessageReceived { from_id, from_name, to, content, timestamp },
    MessageSent { to, content, timestamp, ok_count, total, our_name, our_id },
    PeerConnected { node_id, username },
    PeerDisconnected { node_id, reason },
    SystemNotice(String),
    CommandOutput(String),
    NodeShutdown,
}

enum NodeCommand {
    SendMessage { to, content },
    BroadcastMessage { content },
    Connect { addr },           // 显式 ip:port，node_id 从握手获取
    ConnectById { node_id },    // 注册表查地址（自动判断 ip-only 或 ip:port）
    ConnectByIp { ip },         // 并发探测 DEFAULT_PORTS（IP 发现）
    Chat(Option<String>),
    ListPeers, ListConnected, Help, Quit,
}

struct DisplayLine { kind: LineKind, text: String }
enum LineKind { Incoming, Outgoing, System }
```

#### 2.2.2 ui.rs — TUI 层（Setup + Chat）

**Setup 阶段** — `run_setup(listen_addr) -> Result<(String, String, Terminal)>`

`SetupState` 结构体：
- `node_id: String` / `username: String` — 两个输入字段
- `focus: usize` — 当前激活字段（0=node_id, 1=username）
- `error: Option<String>` — 验证错误提示

`run_setup` 在成功后返回 `Terminal`（仍在 raw mode），由 `run_tui` 接管，避免界面闪烁。

`local_ip()` — 通过向 8.8.8.8:80 "连接"（不实际发包）获取本机出口 IP，用于在 Setup 界面提示用户分享给对方的地址。

**Chat 阶段** — `run_tui(terminal, event_rx, cmd_tx, node_info) -> Result<()>`

`App` 结构体（UI 状态）：
- `messages: VecDeque<DisplayLine>` — 消息历史（最多 500 条）
- `current_chat: Option<String>` — 当前聊天模式目标（UI 层持有）
- `online_peers: Vec<String>` — 用于标题栏
- `scroll_offset: usize` — 0=跟随最新，n=向上滚动 n 行
- `history: Vec<String>` — 命令历史
- `history_idx: Option<usize>` — 历史导航位置

`run_tui()` 主循环：
```
loop {
    terminal.draw(render)
    tokio::select! {
        crossterm keyboard event → handle_key() → dispatch()
        AppEvent from node     → handle_app_event()
    }
}
```

**输入路由逻辑（与原 chat 模式一致）**：
```
按下 Enter
  │
  ├─ current_chat == Some(partner)
  │     ├─ 以 "/" 开头 → 解析命令
  │     │     └─ "/exit" or "/chat" → 清空 current_chat
  │     └─ 其他 → NodeCommand::SendMessage { to: partner }
  │
  └─ current_chat == None → dispatch(cmd_str)
```

#### 2.2.3 P2PNode（node.rs）

`P2PNode` 结构体：
- `event_tx: mpsc::UnboundedSender<AppEvent>` — 向 UI 发送事件
- `connections: ConnectionMap` — 活跃连接表
- `registry: RegistryRef` — 持久化节点信息
- 无 `current_chat`（已移至 UI 层）

`start(cmd_rx, listener)` 方法（消费 self，接受已绑定的 TcpListener）：
1. spawn accept 循环（使用传入的 listener）
2. 连接 initial_peers
3. 循环处理 `NodeCommand`，直到收到 `Quit`

> 注：端口绑定在 `main()` 中完成（`bind_listener(port)`），以便在 Setup 界面显示端口号，再将 listener 传入 `start()`。使用固定的 DEFAULT_PORTS [9000-9004]，跨重启端口保持稳定，registry 地址长期有效。

#### 2.2.4 ConnectionMap / 消息协议 / 注册表

与之前版本相同，参见下方第 3、4 节。

---

## 3. 详细设计

### 3.1 双向握手协议

（与上一版本相同）

```
节点 B（主动连接）                    节点 A（被动接受）
      │                                     │
      │ TCP connect                         │
      │ ──────────────────────────────►     │
      │ Handshake {bob, Bob, :8081}         │
      │ ──────────────────────────────►     │
      │                                     │  registry.upsert(bob→:8081)
      │      Handshake {alice, Alice, :8080} │
      │ ◄──────────────────────────────     │
      │                                     │
      │  registry.upsert(alice→:8080)       │
```

事件发送时机：
- **被连接方** (`handle_incoming_connection`): 收到 Handshake → emit `PeerConnected`
- **连接方** (`handle_read_loop`): 收到 Handshake 回复 → emit `PeerConnected`
- **任一方** (`handle_read_loop`): 收到 Disconnect 或 EOF → emit `PeerDisconnected`

### 3.2 消息协议（Wire Format）

```
+─────────────────────+─────────────────────────+
│   Length (4 bytes)  │   Body (N bytes)        │
│   Big-endian u32    │   bincode serialized     │
+─────────────────────+─────────────────────────+
```

最大消息大小：10 MB（防止恶意大包导致 OOM）

### 3.3 TUI 渲染布局

```
┌──────────────────────────────────────────────────────────┐
│ P2P Chat | Online: alice, bob                    ↑3      │  ← chunks[0]
│                                                          │
│  消息列表 (List widget)                                   │
│  - Incoming: 青色 (Color::Cyan)                          │
│  - Outgoing: 绿色 (Color::Green)                         │
│  - System:   黄色 (Color::Yellow)                        │
│                                                          │
├──────────────────────────────────────────────────────────┤
│ [alice]> _                                               │  ← chunks[1] (3行)
└──────────────────────────────────────────────────────────┘
```

滚动计算：
```
inner_h = area.height - 2  (减去边框)
start = if total ≤ inner_h { 0 }
        else { (total - inner_h) - clamp(scroll_offset, 0, total-inner_h) }
```

### 3.4 任务模型

```
main (tokio::main)
  │
  ├─► tokio::spawn → node.start(cmd_rx)   (后台)
  │     ├─► spawn_accept_loop (spawn)
  │     │     └─► 每个入站连接 (spawn)
  │     │           ├─► handle_write_loop (spawn)
  │     │           └─► handle_read_loop (spawn)
  │     └─► cmd_rx 循环（处理 NodeCommand）
  │
  └─► run_tui(event_rx, cmd_tx)           (前台，阻塞直到退出)
        └─► event_loop
              └─► tokio::select! {
                    crossterm events → handle_key
                    AppEvent → handle_app_event
                  }
```

### 3.5 send 自动连接（get_or_connect）

```
get_or_connect(peer_id)
  │
  ├─ connections.get(peer_id) → Some(tx) → 直接返回
  │
  └─ 未连接
       ├─ registry.get(peer_id) → Some(addr) → connect_to_peer(addr)
       │                                         → 从 connections 取 tx
       └─ registry.get(peer_id) → None → emit SystemNotice，返回 None
```

### 3.6 锁使用规范

1. **不在持有 ConnectionMap 锁时 await** — 先 clone sender，释放锁，再 await
2. **广播时先快照** — 将所有 sender 克隆到 Vec，释放锁，再串行发送
3. **失败连接延迟清理** — 收集到 `failed` 列表，循环结束后统一移除

### 3.7 错误处理策略

| 场景 | 处理方式 |
|------|----------|
| Accept 错误 | 打日志，sleep 100ms，重试 |
| 写循环写入失败 | 从 ConnectionMap 移除，退出循环 |
| 读循环 EOF / 错误 | emit PeerDisconnected，从 ConnectionMap 移除 |
| broadcast 部分失败 | 继续发其余节点，结束后批量清理，emit PeerDisconnected |
| 消息超过 10MB | 返回 Err，连接关闭 |
| 启用 raw mode 失败 | anyhow 传播错误，程序退出（需真实 TTY） |

---

## 3.8 端口绑定与 IP 发现策略

### 3.8.1 端口绑定

`bind_listener(requested_port)` 按以下优先级选择端口：

```
bind_listener(requested_port)
  │
  ├─ requested_port != 0  →  bind("0.0.0.0:{requested_port}")，失败则报错
  │
  └─ requested_port == 0  →  依次尝试 DEFAULT_PORTS [9000, 9001, 9002, 9003, 9004]
        ├─ bind 成功 → 返回（端口跨重启保持稳定）
        └─ 全部失败 → 报错，需手动指定 --port
```

使用固定的知名端口范围，使 registry 中保存的地址跨重启长期有效（同一机器通常绑到相同端口）。

### 3.8.2 IP 发现（connect_by_ip）

当用户执行 `connect <ip>`（纯 IP 地址，无端口）时：

```
connect_by_ip(ip)
  │
  ├─ 为每个 port in DEFAULT_PORTS 并发 spawn 一个任务：
  │     tokio::timeout(200ms, TcpStream::connect(ip:port))
  │     成功 → 将 (stream, addr) 发到 found_rx channel
  │
  ├─ drop(found_tx)  — 确保所有任务失败时 channel 关闭
  │
  ├─ found_rx.recv() → Some((stream, addr))
  │     └─ connect_with_stream(stream, addr)
  │           1. write_msg(Handshake{我方信息})
  │           2. timeout(500ms, read_msg) → Handshake{peer_id, username, listen_addr}
  │           3. registry.upsert(peer)
  │           4. 注册 ConnectionMap，spawn 读写循环
  │           5. emit PeerConnected
  │
  └─ found_rx.recv() → None  →  emit SystemNotice "未找到实例"
```

注：并发探测时未能连接的 5 个任务只是 TCP 连接失败，不产生副作用。

---

## 4. 节点注册表设计

### 4.1 存储格式（known_peers.json）

```json
[
  { "node_id": "alice", "username": "Alice", "address": "127.0.0.1:8080" },
  { "node_id": "bob",   "username": "Bob",   "address": "127.0.0.1:8081" }
]
```

### 4.2 更新时机

| 事件 | 触发方 | 操作 |
|------|--------|------|
| 我主动连接 peer，读循环收到 Handshake 回复 | 连接方 | `upsert(peer)` |
| 对方主动连接我，`handle_incoming_connection` 收到 Handshake | 被连接方 | `upsert(peer)` |

### 4.3 connect 命令的三种形式

```
connect <ip>       → connect_by_ip()：并发探测 DEFAULT_PORTS，握手获取 peer_id
connect <ip:port>  → connect_to_addr()：直接 TCP 拨号，握手获取 peer_id（无需手填）
connect <node_id>  → 注册表查 address：
                       address 为 "ip"      → connect_by_ip(ip)
                       address 为 "ip:port" → connect_to_addr(ip:port)
```

**注册表地址格式**（upsert 时自动 normalize）：

| 对方监听端口 | 存储格式 | 重连方式 |
|------------|---------|---------|
| DEFAULT_PORTS（9000-9004） | `"192.168.1.5"` | IP 发现（自动探测端口） |
| 用户指定端口（如 8888） | `"192.168.1.5:8888"` | 直连精确地址 |

---

## 5. 性能考虑

### 5.1 MPSC 通道大小
每个连接的写通道容量为 32 条消息。高频场景可改为 256。

### 5.2 UI 通道
cmd / event 通道使用 unbounded，避免 UI 渲染抖动导致节点阻塞。

### 5.3 注册表写入
每次 `upsert()` 同步写文件（`std::fs::write`）。节点频繁变动时可改为异步写。

---

## 6. 扩展方向

| 方向 | 说明 |
|------|------|
| 网络发现 | UDP 广播自动发现局域网节点 |
| Keepalive 检测 | 定时发送探活，超时未响应则标记离线 |
| 消息路由 | 通过中间节点转发到不直连的节点 |
| TLS 加密 | tokio-rustls 防中间人攻击 |
| 群组功能 | Group 消息类型 + 成员管理 |
| 文件传输 | 分块传输 + 断点续传 |
| 消息历史持久化 | 本地 SQLite 存储聊天记录 |

# p2p-chat 代码学习文档

> 面向：了解基础 Rust 语法、无实际项目经验的读者  
> 目标：读完本文后能独立理解每个文件的职责、关键 Rust 用法，以及模块间的调用关系

---

## 目录

1. [整体架构](#整体架构)
2. [第一层 — 协议定义：`message.rs`](#第一层--协议定义messagers)
3. [第一层 — 进程内通信：`event.rs`](#第一层--进程内通信eventrs)
4. [第二层 — TCP 帧协议：`connection.rs`](#第二层--tcp-帧协议connectionrs)
5. [第二层 — 节点注册表：`registry.rs`](#第二层--节点注册表registryrs)
6. [第三层 — 网络事件处理：`network.rs`](#第三层--网络事件处理networkrs)
7. [第四层 — 核心状态机：`node.rs`](#第四层--核心状态机norders)
8. [第五层 — 程序入口：`main.rs`](#第五层--程序入口mainrs)
9. [第五层 — 全屏 TUI：`ui.rs`](#第五层--全屏-tuiuirs)
10. [完整数据流：发一条消息](#完整数据流发一条消息)
11. [依赖库速查](#依赖库速查)

---

## 整体架构

```
main.rs          ← 纯接线：解析 CLI、初始化日志、创建对象、连接管道
  ↓
ui.rs            ← 全屏 TUI + 所有用户可见文字（横幅、帮助文字均在此）
  ↕  mpsc channel（双向异步通信）
node.rs          ← P2P 核心：端口绑定策略、连接管理、消息收发、状态维护
  ↓
network.rs       ← 三个 async 网络任务函数
  ↓
connection.rs    ← TCP 帧读写（最底层 I/O）
  ↓
message.rs       ← P2P 消息协议定义（节点间传输的数据格式）

registry.rs      ← 节点注册表（持久化到 known_peers.json）+ DEFAULT_PORTS
event.rs         ← 进程内事件/命令类型（UI ↔ Node 的"语言"）
```

**职责边界**：
- `main.rs`：只做接线，无业务逻辑函数
- `node.rs`：拥有端口策略（`bind_listener`）和所有 P2P 业务
- `ui.rs`：拥有所有用户可见文字（横幅、帮助文字不散落到其他模块）

**依赖方向是单向的**：上层依赖下层，下层不知道上层的存在。`event.rs`、`message.rs`、`registry.rs` 是最底层的定义文件，没有对其他源文件的依赖。

---

## 第一层 — 协议定义：`message.rs`

**职责**：定义两个节点之间通过 TCP 网络传输的消息格式。

### 核心数据结构

```rust
pub enum P2PMessage {
    Handshake { node_id, username, listen_addr, timestamp },
    Direct    { from, to, content, timestamp },
    System    { content, timestamp },
    Keepalive { timestamp },
    Disconnect{ reason, timestamp },
}
```

这是 Rust 的 **enum（枚举）**，但和其他语言的枚举不同——Rust 的 enum 每个变体可以携带不同形状的数据，这叫做**代数数据类型（ADT）**。

| 变体 | 用途 | 关键字段 |
|------|------|---------|
| `Handshake` | 连接时双方互发，交换身份信息 | `listen_addr`：告知对方"下次来哪找我" |
| `Direct` | 聊天消息 | `to: Option<String>`，`None` 表示广播 |
| `System` | 系统通知（对端程序内部发出） | `content` |
| `Keepalive` | 心跳（目前未实际使用） | `timestamp` |
| `Disconnect` | 礼貌断开，附带原因 | `reason` |

### Rust 特性：派生宏（derive）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum P2PMessage { ... }
```

`#[derive(...)]` 是宏，让编译器自动生成代码：
- `Debug`：让你能用 `{:?}` 打印这个类型
- `Clone`：让你能调用 `.clone()` 复制一个值
- `Serialize` / `Deserialize`：来自 `serde` 库，自动生成序列化/反序列化代码

有了 `Serialize` / `Deserialize`，配合 `bincode`（高效二进制格式），就能把消息变成字节流发出去，或从字节流恢复：

```rust
pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::Error> {
    bincode::serialize(self)
}

pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
    bincode::deserialize(bytes)
}
```

### Rust 特性：模式匹配提取所有变体的字段

```rust
pub fn timestamp(&self) -> u64 {
    match self {
        P2PMessage::Handshake  { timestamp, .. }
        | P2PMessage::Direct   { timestamp, .. }
        | P2PMessage::System   { timestamp, .. }
        | P2PMessage::Keepalive{ timestamp, .. }
        | P2PMessage::Disconnect{ timestamp, .. } => *timestamp,
    }
}
```

`..` 表示"忽略其余字段"。`|` 把多个模式合并到同一个分支。`*timestamp` 是解引用（`timestamp` 是 `&u64`，`*` 取出 `u64` 值）。

---

## 第一层 — 进程内通信：`event.rs`

**职责**：定义 UI 和 Node 在**同一进程内**互相传递的消息类型。

> **重要区别**：  
> `P2PMessage` = 跨网络，不同机器的进程之间  
> `AppEvent` / `NodeCommand` = 进程内，UI 线程和 Node 线程之间

### AppEvent：Node → UI

```rust
pub enum AppEvent {
    MessageReceived { from_id, from_name, to, content, timestamp },
    MessageSent     { to, content, timestamp, ok_count, total, our_name, our_id },
    PeerConnected   { node_id, username },
    PeerDisconnected{ node_id, reason },
    SystemNotice(String),    // 元组变体（不带字段名）
    CommandOutput(String),
    NodeShutdown,
}
```

Node 每次有事情要告知 UI（收到消息、节点连接/断开等），就往 `event_tx` channel 里发一个 `AppEvent`。

### NodeCommand：UI → Node

```rust
pub enum NodeCommand {
    SendMessage { to: String, content: String },
    BroadcastMessage { content: String },
    Connect { addr: String },           // connect <ip:port>
    ConnectById { node_id: String },    // connect <node_id>（registry 查找）
    ConnectByIp { ip: String },         // connect <ip>（探测默认端口）
    Chat(Option<String>),
    ListPeers,
    ListConnected,
    Quit,
}
```

用户在 UI 输入命令后，`dispatch()` 函数把字符串解析成对应的 `NodeCommand`，通过 `cmd_tx` channel 发给 Node。

> **注意**：`help` 命令不在这里——帮助文字是纯 UI 内容，`dispatch()` 直接把 `HELP_LINES` 写入消息列表，不需要经过 Node。

**为什么用 channel + enum 而不是直接调用方法？**  
因为 UI 和 Node 运行在不同的 async 任务中，不能直接调用彼此的方法（所有权/借用规则不允许）。Channel 是 Rust async 编程中跨任务通信的标准方式。

### DisplayLine / LineKind

```rust
pub enum LineKind { Incoming, Outgoing, System }

pub struct DisplayLine {
    pub kind: LineKind,
    pub text: String,
}
```

UI 渲染消息窗格时，每一行都包装成 `DisplayLine`，根据 `kind` 决定颜色：青色（收到）、绿色（发出）、黄色（系统）。

---

## 第二层 — TCP 帧协议：`connection.rs`

**职责**：解决 TCP 字节流没有消息边界的问题，实现"帧"的读写；提供共享连接表的类型定义。

### 为什么需要"帧"

TCP 是字节流协议。如果你连续发两条消息 `[A, B, C]` 和 `[D, E]`，接收方可能一次收到 `[A, B, C, D, E]`，不知道第一条消息在哪里结束。

解决方案：**长度前缀帧**。每条消息前面先发 4 字节表示消息长度：

```
┌──────────────────┬─────────────────────────┐
│  length  (4 字节) │  body  (length 字节)    │
│  大端序 u32       │  bincode 序列化的消息   │
└──────────────────┴─────────────────────────┘
```

发送端：
```rust
pub async fn write_msg(writer: &mut OwnedWriteHalf, message: P2PMessage) -> Result<()> {
    let bytes = message.to_bytes()?;
    // 先写 4 字节长度（大端序）
    writer.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
    // 再写内容
    writer.write_all(&bytes).await?;
    Ok(())
}
```

接收端：
```rust
pub async fn read_msg(reader: &mut OwnedReadHalf) -> Result<Option<P2PMessage>> {
    let mut len_buf = [0u8; 4];
    if reader.read_exact(&mut len_buf).await.is_err() {
        return Ok(None);  // 连接关闭了（EOF）
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    Ok(Some(P2PMessage::from_bytes(&buf)?))
}
```

`read_exact` 是"读满 N 字节，不够就等"——它不会返回半条消息。

### 共享连接表

#### `Connection` 与 `mpsc::Sender`

```rust
pub struct Connection {
    pub tx: mpsc::Sender<P2PMessage>,
}
```

`mpsc` 是 **Multi-Producer, Single-Consumer**（多生产者、单消费者）的缩写。创建时会同时得到一对端点：

```rust
let (tx, rx) = mpsc::channel::<P2PMessage>(32);
//  发送端    接收端
```

| | `tx`（Sender，发送端） | `rx`（Receiver，接收端） |
|---|---|---|
| 可以有几个 | 多个（可以 `.clone()` 复制） | 只有一个 |
| 操作 | `tx.send(msg).await` | `rx.recv().await` |
| 持有者 | 存在 `Connection.tx` 里，谁想发消息谁拿一份 | 只有 `handle_write_loop` 持有 |

`Connection` 结构体里只存 `tx`（发送端），是因为程序的其他部分只需要"往这个 peer 发消息"这个能力。实际写 TCP 的工作全交给 `handle_write_loop` 独自完成。

**为什么不直接操作 TCP，而要绕一圈用 channel？**

因为多个地方可能**同时**想给同一个 peer 发消息（例如广播时并发发给 3 个 peer），直接共享 TCP 写端会有并发冲突，需要加锁。用 channel 做缓冲后，写端只有 `handle_write_loop` 一个任务持有，天然串行，不需要加任何锁：

```
node.rs: broadcast()  →  tx.clone().send(msg)  ─┐
node.rs: send_msg()   →  tx.clone().send(msg)  ─┤──▶ [channel 队列，最多 32 条] ──▶ write_loop ──▶ TCP
连接握手处           →  tx.clone().send(msg)  ─┘         （只有这一个消费者）
```

`channel(32)` 里的 `32` 是缓冲容量：队列里最多积压 32 条消息。如果满了，`send` 会挂起等待（这叫**背压**，防止内存无限增长）。

**`tx` 和 `rx` 在哪里分开的**

`tx` 和 `rx` 在 `node.rs` 的 `connect_with_stream()` 里同时创建，然后立刻**分道扬镳**：

```rust
// node.rs:497-508（连接握手完成后）
let (tx, rx) = mpsc::channel::<P2PMessage>(32);  // ① 创建一对端点

self.connections.lock().await
    .insert(peer_id.clone(), Connection { tx });  // ② tx 存入 ConnectionMap

tokio::spawn(handle_write_loop(
    write_half,   // TCP 写端
    rx,           // ③ rx 交给后台任务，永远 move 进去了
    ...
));
```

三行代码之后，`tx` 和 `rx` 就再也没有在同一个地方出现过。它们之间唯一的联系是 channel 内部的消息队列。

```
创建时（node.rs）：
  let (tx, rx) = mpsc::channel(32)
                  │              │
                  ▼              └──────────────────────────────────┐
  ConnectionMap["alice"]                                            │
    = Connection { tx }                                             ▼
                  │                               tokio::spawn(handle_write_loop(rx, tcp_writer))
                  │                                                 │
                  │                                       后台任务永远在跑
                  │                                                 │
之后发消息（任何地方）：                                            │
  conn.tx.send(msg).await ──────▶ [channel 队列，最多 32 条] ──▶ rx.recv().await
                                                                    │
                                                                    ▼
                                                             write_msg() → TCP 网络
```

**为什么 `Connection` 里只存 `tx`，不存 `rx`？**

因为 `rx` 被 `move` 进 `handle_write_loop` 的 async 块后，**所有权永久转移**进去了——Rust 的所有权规则保证同一时刻只能有一个拥有者。`rx` 进了后台任务就再也出不来，其他地方根本拿不到它，也不需要存。

`tx`（`Sender`）则实现了 `Clone`，可以复制出无数份，每一份都是同一个队列的入口。存一份在 `ConnectionMap` 里，谁想发消息谁 clone 一份：

```rust
// 典型用法
let tx = conn.tx.clone();      // 克隆发送端（共享同一个队列入口，不复制队列本身）
drop(conns);                   // 立即释放 ConnectionMap 的锁
tx.send(msg).await?;           // 发送时已不持有锁，不阻塞其他操作
```

**`handle_write_loop` 如何处理收到的消息**

```rust
// network.rs
pub async fn handle_write_loop(
    mut writer: OwnedWriteHalf,          // TCP 写端（独占）
    mut rx: mpsc::Receiver<P2PMessage>,  // channel 接收端（独占）
    peer_id: String,
    connections: ConnectionMap,
) {
    while let Some(msg) = rx.recv().await {  // 阻塞等待，直到有消息或 channel 关闭
        if let Err(e) = write_msg(&mut writer, msg).await {
            // 写 TCP 失败（对方断了）→ 从 ConnectionMap 移除，退出循环
            connections.lock().await.remove(&peer_id);
            break;
        }
    }
    // while let 退出：channel 关闭（所有 tx 都被 drop）→ 任务自然结束
}
```

`while let Some(msg) = rx.recv().await` 的含义：
- 有消息 → `Some(msg)`，进入循环体处理
- channel 关闭（所有 `tx` 都被 drop）→ 返回 `None`，`while let` 退出，任务结束
- 写 TCP 出错 → `break` 强制退出

所以 `handle_write_loop` 的生命周期和连接完全绑定：连接活着它就跑，连接断了（`tx` 从 `ConnectionMap` 被移除并 drop）它就自动停止。

#### `ConnectionMap`

```rust
pub type ConnectionMap = Arc<Mutex<HashMap<String, Connection>>>;
//                        ^^^  ^^^^^^^^  ^^^^^^^^^^^^^^^^^^^^^^^
//                        原子 互斥锁    node_id → Connection
//                        引用计数
```

`ConnectionMap` 是整个项目中最重要的共享状态，三层包装各有用途：

| 包装层 | 作用 |
|--------|------|
| `HashMap<String, Connection>` | 用 node_id 查找某个 peer 的发送通道 |
| `Mutex<...>` | 互斥锁。同一时刻只允许一个任务修改，防止数据竞争 |
| `Arc<...>` | 原子引用计数。允许多个 async 任务**同时持有**同一份数据的"所有权" |

使用时：
```rust
// 加锁，获得 MutexGuard（离开作用域自动释放锁）
let mut conns = connections.lock().await;
conns.insert("alice".to_string(), Connection { tx });
// 离开这个块，锁自动释放
```

---

## 第二层 — 节点注册表：`registry.rs`

**职责**：记住曾经连接过的节点，程序重启后仍能重连。

### 数据结构

```rust
pub struct KnownPeer {
    pub node_id:  String,
    pub username: String,
    pub address:  String,  // "192.168.1.5" 或 "192.168.1.5:8888"
}

pub struct PeerRegistry {
    path:  PathBuf,                      // JSON 文件路径（默认 known_peers.json）
    peers: HashMap<String, KnownPeer>,   // 内存中的数据
}

pub type RegistryRef = Arc<Mutex<PeerRegistry>>;  // 和 ConnectionMap 同样的模式
```

### 地址存储策略

`address` 字段有两种存储格式，取决于端口是否是默认端口（9000-9004）：

| peer 端口 | 存储内容 | 重连时 |
|-----------|---------|--------|
| 9000-9004（默认） | `"192.168.1.5"`（只存 IP）| `connect_by_ip()` 并发探测 |
| 自定义（如 8888） | `"192.168.1.5:8888"`（完整地址）| `connect_to_addr()` 直连 |

这个转换在 `normalize_peer_addr()` 中完成：
```rust
pub fn normalize_peer_addr(addr_str: &str) -> String {
    if let Ok(sock) = addr_str.parse::<SocketAddr>() {
        if DEFAULT_PORTS.contains(&sock.port()) {
            return sock.ip().to_string();  // 默认端口 → 只存 IP
        }
    }
    addr_str.to_string()  // 自定义端口 → 保留完整 ip:port
}
```

`if let Ok(sock) = expr` 是 Rust 的**条件模式匹配**：如果 `parse()` 返回 `Ok`，就把里面的值绑定到 `sock`；如果是 `Err` 则跳过整个 if 块。

### 持久化

每次 `upsert()` 后立即写盘，不缓存：
```rust
pub fn upsert(&mut self, peer: KnownPeer) {
    self.peers.insert(peer.node_id.clone(), peer);
    self.flush();  // 立即写 JSON 文件
}
```

`flush()` 把 `HashMap` 的值收集成 `Vec`，用 `serde_json::to_string_pretty` 序列化成格式化 JSON，写到文件。

---

## 第三层 — 网络事件处理：`network.rs`

**职责**：三个长期运行的 async 函数，负责单个 TCP 连接的完整生命周期。

### `handle_incoming_connection`（服务端握手）

当 `TcpListener::accept()` 接受到新连接时调用，执行服务端握手流程：

```
1. read_msg() → 必须是 Handshake，否则报错
2. registry.upsert(normalize_peer_addr(peer_addr))  ← 记录对方
3. tx.send(我们的 Handshake)                         ← 回复握手
4. spawn handle_write_loop(write_half, rx, ...)      ← 后台写任务
5. connections.insert(peer_id, Connection { tx })    ← 注册连接
6. event_tx.send(PeerConnected)                      ← 通知 UI
7. spawn handle_read_loop(read_half, ...)             ← 后台读任务
```

注意**先注册连接再启动读循环**，这样 `handle_read_loop` 开始工作时，`ConnectionMap` 里已经有这个 peer 了。

### `handle_read_loop`（持续读取消息）

每个连接一个，`loop` 不断读取消息：

```rust
loop {
    match read_msg(&mut reader).await {
        Ok(Some(P2PMessage::Handshake { .. })) => {
            // 外连路径（我们主动连接时）收到对方的握手回复
            // 更新 registry，通知 UI PeerConnected
        }
        Ok(Some(P2PMessage::Direct { from, to, content, timestamp })) => {
            // 收到聊天消息，查询 from 的显示名，通知 UI
        }
        Ok(Some(P2PMessage::Disconnect { reason, .. })) => {
            connections.remove(peer_id);
            event_tx.send(PeerDisconnected);
            break;  // 退出循环，任务结束
        }
        Ok(None) => { /* 对方关闭连接（EOF） */ break; }
        Err(e)   => { /* 读取错误 */ break; }
        ...
    }
}
```

`break` 退出 loop 后，async 任务自然结束，tokio 自动回收资源。

### `handle_write_loop`（持续写入消息）

```rust
while let Some(msg) = rx.recv().await {
    write_msg(&mut writer, msg).await?;
}
// rx 关闭（Connection 被 drop）时，while let 退出，任务结束
```

想给某个 peer 发消息，只需往 `Connection.tx` 发，这个 loop 就会自动把消息写到 TCP 流。这是**生产者-消费者**模式：多个地方生产消息（往 `tx` 发），一个消费者（write_loop）写出去，天然串行，不用加锁。

### 时间工具

```rust
pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

pub fn format_time(unix_secs: u64) -> String {
    let s = unix_secs % 86400;  // 取当天的秒数
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}
```

消息时间戳存的是 Unix 秒数，显示时转为 `HH:MM:SS`（UTC）。

---

## 第四层 — 核心状态机：`node.rs`

**职责**：整个应用的"大脑"——**端口绑定**、接受连接、处理命令、管理连接生命周期、收发消息。

> 端口绑定策略（`bind_listener()`）也放在这里，因为"默认端口范围"是 P2P 网络的业务规则，不是程序入口该关心的事。`main.rs` 只需调用 `node::bind_listener(args.port)`，不感知内部细节。

### 结构体定义

```rust
pub struct P2PNode {
    pub node_id:       String,
    pub username:      String,
    pub listening_addr: SocketAddr,
    pub initial_peers: Vec<String>,        // --peers 参数
    pub connections:   ConnectionMap,      // 共享：当前在线连接
    pub registry:      RegistryRef,        // 共享：持久化节点信息
    event_tx: mpsc::UnboundedSender<AppEvent>,  // 向 UI 发事件（私有）
}
```

`event_tx` 是私有的（无 `pub`），node 只能通过 `emit()` 方法发事件：
```rust
fn emit(&self, ev: AppEvent) {
    let _ = self.event_tx.send(ev);  // 忽略错误（UI 退出时 channel 会关闭）
}
```

`let _ = ...` 是 Rust 中故意忽略返回值的惯用写法。

### `start()` 的三层结构

`start()` 拆成三个层次，职责分明：

```rust
pub async fn start(self, mut cmd_rx: ..., listener: TcpListener) -> Result<()> {
    self.on_start(listener).await;          // ① 初始化

    while let Some(cmd) = cmd_rx.recv().await {
        if self.handle_command(cmd).await { // ② 单条命令处理，返回 true = 退出
            break;
        }
    }

    info!("Node shut down");
    Ok(())
}
```

**`on_start()`**：初始化三件事——通知 UI 已加载多少已知节点、启动后台 accept loop、连接 `--peers` 参数指定的初始节点。`--peers` 统一走 `connect_to_addr()`，peer_id 从握手中获取（和其他连接方式行为一致）。

**`handle_command()`**：处理单条 `NodeCommand`，返回 `bool`（`true` 表示收到 Quit，应退出循环）：

```rust
async fn handle_command(&self, cmd: NodeCommand) -> bool {
    match cmd {
        NodeCommand::Quit => {
            self.shutdown().await;
            self.emit(AppEvent::NodeShutdown);
            true   // ← 通知 start() 退出 while 循环
        }
        NodeCommand::SendMessage { to, content } => {
            self.send_message(&to, content).await;
            false
        }
        NodeCommand::Connect { addr }     => { self.connect_to_addr(&addr).await; false }
        NodeCommand::ConnectByIp { ip }   => { self.connect_by_ip(&ip).await; false }
        NodeCommand::ConnectById { .. }   => { /* registry 查地址 → 重连 */ false }
        NodeCommand::Chat(Some(id))       => { /* 未连接则自动重连 */ false }
        NodeCommand::ListConnected        => { /* 遍历 connections 输出 */ false }
        NodeCommand::ListPeers            => { /* 遍历 registry 输出 */ false }
        ...
    }
}
```

`while let Some(cmd) = cmd_rx.recv().await`：每次 await 等待 UI 发来命令，channel 关闭时自动结束循环。

### 三种连接方式

```
用户输入 "connect 192.168.1.5"        → ConnectByIp { ip }
                                         ↓
                                   connect_by_ip(ip)
                                         ↓
                  并发 spawn 5 个任务，各尝试 9000-9004 端口（200ms 超时）
                                         ↓
                  第一个成功的通过 mpsc channel 返回 (TcpStream, SocketAddr)
                                         ↓
                                 connect_with_stream(stream, addr)

用户输入 "connect 192.168.1.5:9000"   → Connect { addr }
                                         ↓
                                   connect_to_addr(addr)
                                         ↓
                             TcpStream::connect(addr) → connect_with_stream(...)

用户输入 "connect alice"               → ConnectById { node_id: "alice" }
                                         ↓
                          registry.get("alice") → address（IP 或 ip:port）
                                         ↓
                               connect_by_addr_or_ip(address)
                                   /                  \
                        "ip:port"能解析              只是 "ip"
                        connect_to_addr()           connect_by_ip()
```

### `connect_with_stream()`：握手的客户端侧

拿到已建立的 `TcpStream` 后，完成握手，学到对方的 node_id：

```rust
async fn connect_with_stream(&self, socket: TcpStream, addr: SocketAddr) -> Result<()> {
    let (mut read_half, mut write_half) = socket.into_split();

    // 1. 发我们的 Handshake（不需要提前知道对方 node_id）
    write_msg(&mut write_half, P2PMessage::new_handshake(
        self.node_id.clone(), self.username.clone(), self.listening_addr.to_string(),
    )).await?;

    // 2. 等对方的 Handshake 回复，最多等 500ms
    let reply = tokio::time::timeout(
        Duration::from_millis(500),
        read_msg(&mut read_half),
    ).await
    .map_err(|_| anyhow!("Handshake timeout"))?  // timeout 返回 Err
    .map_err(|e| anyhow!("Read error: {}", e))?;  // read_msg 返回 Err

    match reply {
        Some(P2PMessage::Handshake { node_id: peer_id, username, listen_addr, .. }) => {
            // 3. 现在知道对方的 node_id 了
            registry.upsert(KnownPeer {
                address: normalize_peer_addr(&listen_addr),  // 地址归一化
                ...
            });
            // 4. 注册连接，启动 read/write loop，通知 UI
        }
        _ => Err(anyhow!("Expected Handshake, got something else"))
    }
}
```

`tokio::time::timeout(duration, future)` 返回 `Result<T, Elapsed>`：超时返回 `Err(Elapsed)`，否则返回内层 Future 的结果。

### `get_or_connect()`：懒重连

`send_message()` 在发消息前会调用这个函数，实现"如果断线了就自动重连"：

```rust
async fn get_or_connect(&self, peer_id: &str) -> Result<Option<Sender<P2PMessage>>> {
    // 快速路径：已在线
    {
        let conns = self.connections.lock().await;
        if let Some(conn) = conns.get(peer_id) {
            return Ok(Some(conn.tx.clone()));
        }
    }  // ← 锁在这里释放

    // 慢速路径：查 registry，尝试重连
    let addr_str = self.registry.lock().await
        .get(peer_id)
        .map(|p| p.address.clone());

    match addr_str {
        Some(addr) => {
            self.connect_by_addr_or_ip(&addr).await?;
            // 重连后再查一次
            Ok(self.connections.lock().await.get(peer_id).map(|c| c.tx.clone()))
        }
        None => {
            self.emit(AppEvent::SystemNotice("Peer unknown...".into()));
            Ok(None)
        }
    }
}
```

注意：先加锁读数据，用一对 `{}` 限制作用域让锁**立即释放**，然后再做可能阻塞的操作（重连）。这是避免死锁的重要技巧。

### 优雅退出 `shutdown()`

```rust
async fn shutdown(&self) {
    {
        let conns = self.connections.lock().await;
        for (peer_id, conn) in conns.iter() {
            // 往每个 peer 的写通道发 Disconnect 消息
            let _ = conn.tx.send(P2PMessage::new_disconnect("Node shutting down".into())).await;
        }
    }
    tokio::time::sleep(Duration::from_millis(200)).await;  // 等消息发出去
    self.registry.lock().await.flush();  // 最终写盘
}
```

---

## 第五层 — 程序入口：`main.rs`

**职责**：纯粹的"接线"——解析 CLI、初始化日志、创建对象、连接管道、按顺序启动。文件中只有 `Args` 结构体和 `main()` 函数，**不包含任何业务逻辑函数**。

### CLI 参数（clap 库）

```rust
#[derive(Parser, Debug)]
#[command(name = "p2p-chat")]
struct Args {
    #[arg(short = 'P', long, default_value = "0")]
    port: u16,

    #[arg(short, long, num_args = 0..)]
    peers: Vec<String>,

    #[arg(long, default_value = "known_peers.json")]
    peers_file: String,
}
```

`#[derive(Parser)]` 是 `clap` 库的宏，自动根据结构体字段生成 CLI 参数解析代码。运行时 `Args::parse()` 就能拿到解析结果。

### 启动流程

```rust
#[tokio::main]  // 这个属性宏：让 main 变成 async，并启动 Tokio 异步运行时
async fn main() -> Result<()> {
    // 1. 日志输出到文件（不干扰 TUI）
    let log_file = OpenOptions::new().create(true).append(true).open("p2p-chat.log")?;
    tracing_subscriber::fmt().with_writer(log_file).with_ansi(false).init();

    // 2. 解析 CLI 参数
    let args = Args::parse();

    // 3. 绑定端口（端口策略由 node.rs 负责，main.rs 只管调用）
    let listener = node::bind_listener(args.port).await?;
    let listen_addr = listener.local_addr()?;

    // 4. Setup TUI：收集 node_id 和 username
    //    返回的 terminal 保持 raw mode，聊天 TUI 复用它（无闪屏）
    let (node_id, username, terminal) = ui::run_setup(listen_addr).await?;

    // 5. 创建两个 channel（双向通信管道）
    let (cmd_tx, cmd_rx)     = mpsc::unbounded_channel::<NodeCommand>();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<AppEvent>();
    //    UI 持有 cmd_tx   Node 持有 cmd_rx     Node 持有 event_tx   UI 持有 event_rx

    // 6. 创建 Node，在后台任务启动（clone 是因为 run_tui 也需要这两个值）
    let node = P2PNode::new(node_id.clone(), username.clone(), listen_addr, args.peers, &args.peers_file, event_tx)?;
    let node_handle = tokio::spawn(async move {
        node.start(cmd_rx, listener).await
    });

    // 7. 运行聊天 TUI（阻塞，直到用户退出）
    //    启动横幅由 ui.rs 内部用 listen_addr / node_id / username 自行拼装
    ui::run_tui(terminal, event_rx, cmd_tx, listen_addr, node_id, username).await?;

    // 8. 等 Node 优雅退出（最多 2 秒）
    let _ = tokio::time::timeout(Duration::from_secs(2), node_handle).await;
    Ok(())
}
```

`tokio::spawn` 把一个 async 块放到后台运行，返回 `JoinHandle`（可以用来等待任务结束）。UI (`run_tui`) 和 Node (`start`) 是**并发**运行的，通过 channel 通信。

---

## 第五层 — 全屏 TUI：`ui.rs`

**职责**：用 `ratatui` 库绘制全屏界面，用 `crossterm` 库读取键盘输入。**拥有所有用户可见的文字**——启动横幅、帮助文字均在此定义，不散落到其他模块。

### 界面模式

`ui.rs` 有两个独立的界面：

1. **Setup 界面**（`run_setup()`）：收集 node_id 和 username
2. **聊天界面**（`run_tui()`）：显示消息、输入命令

`run_setup()` 结束后**不关闭 terminal**，直接把它传给 `run_tui()` 继续用——这样切换时屏幕不会闪烁。

### `run_tui()` 的签名与横幅构建

```rust
pub async fn run_tui(
    mut terminal: Term,
    event_rx: mpsc::UnboundedReceiver<AppEvent>,
    cmd_tx: mpsc::UnboundedSender<NodeCommand>,
    listen_addr: SocketAddr,   // ← 从 main.rs 传入结构化数据
    node_id: String,
    username: String,
) -> Result<()> {
    // 横幅在这里拼，main.rs 不需要关心格式
    let port = listen_addr.port();
    let lan_ip = local_ip();   // local_ip() 也在 ui.rs 里
    let node_info = [
        format!("Node: {} ({})", node_id, username),
        format!("Listening → local: 127.0.0.1:{}   LAN: {}:{}", port, lan_ip, port),
        "Share the LAN address with peers so they can connect to you.".to_string(),
    ];
    ...
}
```

### 帮助文字：`HELP_LINES` 常量

帮助文字定义为文件顶部的常量，`dispatch()` 的 `"help"` 分支直接写入 app，不发 `NodeCommand`：

```rust
const HELP_LINES: &[&str] = &[
    "Commands:",
    "  connect <ip>       - Connect by IP (probes ports 9000-9004 concurrently)",
    ...
];

// dispatch() 里
"help" => {
    for line in HELP_LINES {
        app.system(*line);   // 直接写消息列表，不经过 Node
    }
}
```

### App 状态结构体

```rust
struct App {
    messages:     VecDeque<DisplayLine>,  // 消息列表（最多 500 条，超出删最旧的）
    input:        String,                 // 当前输入框内容
    current_chat: Option<String>,         // Some(node_id) = 聊天模式，None = 命令模式
    online_peers: Vec<String>,            // 标题栏显示的在线节点
    scroll_offset: usize,                 // 0 = 跟随最新，N = 向上滚动 N 行
    history:      Vec<String>,            // 命令历史
    history_idx:  Option<usize>,          // 当前历史导航位置
}
```

`VecDeque`（双端队列）适合从头删、从尾加的场景，比 `Vec` 高效。

### 主循环：`tokio::select!`

聊天界面的核心是 `chat_loop()`，它同时监听两个事件源：

```rust
loop {
    terminal.draw(|f| render_chat(f, app))?;  // 重绘界面

    tokio::select! {
        // 分支1：键盘事件
        maybe = keys.next() => {
            match maybe {
                Some(Ok(Event::Key(key))) => {
                    if handle_key(app, key, &cmd_tx)? { break; }  // true = 要退出
                }
                ...
            }
        }

        // 分支2：来自 Node 的 AppEvent
        msg = event_rx.recv() => {
            match msg {
                Some(ev) => {
                    if handle_app_event(app, ev) { break; }  // NodeShutdown 时 break
                }
                None => break,  // channel 关闭
            }
        }
    }
}
```

`tokio::select!` 同时等待多个 async 操作，**哪个先完成就先处理哪个**，然后重新循环。这样键盘输入和网络事件都能及时响应，不会互相阻塞。

### 命令分发：`dispatch()`

用户按 Enter 后，输入经过 `route_input()` → `dispatch()` 解析：

```rust
fn dispatch(app: &mut App, cmd: &str, cmd_tx: &Sender<NodeCommand>) -> Result<bool> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();

    match parts[0] {
        "connect" => {
            if parts[1].parse::<IpAddr>().is_ok() {         // 纯 IP 地址
                cmd_tx.send(NodeCommand::ConnectByIp { ip: parts[1].into() });
            } else if parts[1].parse::<SocketAddr>().is_ok() {  // ip:port
                cmd_tx.send(NodeCommand::Connect { addr: parts[1].into() });
            } else {                                          // node_id
                cmd_tx.send(NodeCommand::ConnectById { node_id: parts[1].into() });
            }
        }
        "chat" => { /* 更新 app.current_chat，发 NodeCommand::Chat */ }
        "send" => { /* parts[2..].join(" ") 作为消息内容 */ }
        "broadcast" => { ... }
        "quit" | "exit" => return Ok(true),  // 返回 true 通知调用者退出
        _ => { app.system(format!("Unknown command '{}'", parts[0])); }
    }
    Ok(false)
}
```

`parts[1].parse::<IpAddr>().is_ok()` 用 Rust 的类型推断——`.parse::<IpAddr>()` 尝试把字符串解析成 IP 地址，`.is_ok()` 检查是否成功，不关心具体值。这是判断字符串类型的简洁写法。

### 聊天模式下的输入路由

```rust
fn route_input(app: &mut App, input: String, cmd_tx: ...) -> Result<bool> {
    if let Some(ref partner) = app.current_chat.clone() {
        // 聊天模式下：
        if let Some(cmd_str) = input.strip_prefix('/') {
            // 以 / 开头 → 当命令处理（如 /list, /exit）
            if cmd_str == "exit" { app.current_chat = None; }
            else { dispatch(app, cmd_str, cmd_tx)?; }
        } else {
            // 普通文字 → 直接发消息给当前聊天对象
            cmd_tx.send(NodeCommand::SendMessage { to: partner.clone(), content: input });
        }
        return Ok(false);
    }
    // 非聊天模式：所有输入都当命令处理
    dispatch(app, &input, cmd_tx)
}
```

### ratatui 布局简介

```rust
fn render_chat(f: &mut Frame, app: &App) {
    // 把屏幕竖向分成两块：消息区（撑满剩余空间）+ 输入栏（3行固定高）
    let chunks = Layout::vertical([
        Constraint::Min(3),      // 消息区：至少 3 行，尽量撑满
        Constraint::Length(3),   // 输入栏：固定 3 行
    ]).split(f.area());

    // 渲染消息列表（带滚动逻辑）
    f.render_widget(List::new(items).block(Block::default().borders(Borders::ALL)), chunks[0]);

    // 渲染输入栏
    f.render_widget(Paragraph::new(prompt).block(...), chunks[1]);

    // 设置光标位置
    f.set_cursor_position((cx, cy));
}
```

ratatui 是**立即模式 GUI**：每一帧都从头重绘，不保留上一帧的状态。`Frame` 是当前帧的画布，`render_widget` 往上画组件，`draw()` 调用结束后一次性刷新到终端。

---

## 完整数据流：发一条消息

以用户在聊天模式输入 `Hello!` 按 Enter 为例，追踪完整路径：

```
[用户按 Enter]
    ↓
ui.rs: chat_loop() 的 select! 收到 KeyCode::Enter
    ↓
ui.rs: handle_key() → route_input()
    处于聊天模式，不以 / 开头
    ↓
cmd_tx.send(NodeCommand::SendMessage { to: "alice", content: "Hello!" })
    ↓
node.rs: 主循环收到 NodeCommand::SendMessage
    ↓
node.rs: send_message("alice", "Hello!")
    ↓
node.rs: get_or_connect("alice")
    → connections 里有 alice → 返回 conn.tx
    ↓
conn.tx.send(P2PMessage::Direct { from: "bob", to: Some("alice"), content: "Hello!", .. })
    ↓
[进程内 mpsc channel]
    ↓
network.rs: handle_write_loop 的 rx.recv() 收到消息
    ↓
connection.rs: write_msg() → 写 4字节长度 + bincode 内容到 TCP
    ↓
[TCP 网络传输]
    ↓
对方机器的 connection.rs: read_msg() 读出帧，反序列化
    ↓
network.rs: handle_read_loop 的 match 匹配到 P2PMessage::Direct
    查 registry 得到 "bob" 的 username
    ↓
event_tx.send(AppEvent::MessageReceived { from_id: "bob", from_name: "Bob", .. })
    ↓
[进程内 mpsc channel]
    ↓
ui.rs: chat_loop() 的 select! 收到 AppEvent
    ↓
ui.rs: handle_app_event() → 格式化文本，app.push(DisplayLine { kind: Incoming, .. })
    ↓
下一帧 render_chat() 重绘，消息出现在界面上
```

同时，我方 UI 也会显示自己发出的消息（绿色）：
```
node.rs: send_message() 发送成功后
event_tx.send(AppEvent::MessageSent { to: Some("alice"), content: "Hello!", .. })
    ↓
ui.rs: handle_app_event() 把它加入消息列表，kind: Outgoing（绿色）
```

---

## 依赖库速查

| 库 | 用途 | 主要使用位置 |
|----|------|------------|
| `tokio` | 异步运行时，提供 `spawn`、`select!`、`mpsc`、`TcpListener`、`timeout` | 几乎所有文件 |
| `serde` | 序列化框架，提供 `Serialize` / `Deserialize` derive 宏 | `message.rs`、`registry.rs` |
| `bincode` | 高效二进制序列化格式 | `message.rs`（`to_bytes` / `from_bytes`） |
| `serde_json` | JSON 序列化，用于读写 `known_peers.json` | `registry.rs` |
| `anyhow` | 统一错误处理，`Result<T>` = `Result<T, anyhow::Error>` | 所有返回 `Result` 的函数 |
| `ratatui` | 全屏 TUI 框架：布局、组件、渲染 | `ui.rs` |
| `crossterm` | 跨平台终端控制：raw mode、键盘事件、光标 | `ui.rs` |
| `clap` | CLI 参数解析，`#[derive(Parser)]` | `main.rs` |
| `tracing` | 结构化日志，`info!`、`warn!`、`error!` 宏 | `node.rs`、`network.rs` 等 |
| `futures` | `StreamExt`，让键盘事件流支持 `.next().await` | `ui.rs` |

### `anyhow::Result` 和 `?` 运算符

整个项目的函数几乎都返回 `Result<T>` 或 `Result<()>`，使用 `?` 传播错误：

```rust
async fn connect_to_addr(&self, addr_str: &str) -> Result<()> {
    let addr: SocketAddr = addr_str
        .parse()
        .map_err(|e| anyhow!("Invalid address: {}", e))?;  // parse 失败 → 立即返回 Err
    let stream = TcpStream::connect(&addr).await?;          // 连接失败 → 立即返回 Err
    self.connect_with_stream(stream, addr).await?;          // 握手失败 → 立即返回 Err
    Ok(())
}
```

`?` 等价于：如果是 `Err`，把错误向上返回；如果是 `Ok`，取出里面的值继续执行。

---

*本文档随学习过程持续更新。有疑问请直接提问，文档会同步补充相关内容。*

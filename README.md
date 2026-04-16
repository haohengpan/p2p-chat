# P2P Chat

一个纯点对点的聊天应用程序，无需中央服务器，使用 Rust + Tokio 实现，带有基于 ratatui 的全屏 TUI 界面。

## 特性

- 真正的 P2P 架构，无服务器依赖
- **零配置启动**：直接运行即可，自动绑定端口，通过交互界面输入用户信息
- **全屏 TUI**：Setup 设置界面 + 聊天界面（消息区 + 输入栏 + 标题栏）
- **节点注册表**：连接过的节点自动记录到 `known_peers.json`，下次可直接用 `node_id` 重连
- 私聊模式（`chat <node_id>`）与广播（`broadcast`）
- 消息显示 UTC 时间戳和发送人名称
- `send` / `chat` 自动尝试重连已知离线节点
- 优雅关闭，断开前通知所有对端
- 日志输出到 `p2p-chat.log`，不干扰 TUI

---

## 快速开始

### 1. 安装 Rust
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

### 2. 编译项目
```bash
cargo build --release
```

### 3. 直接启动，无需参数

```bash
cargo run --release
```

或使用编译好的二进制：

```bash
./target/release/p2p-chat
```

启动后会显示 **Setup 设置界面**：

```
╔══════════════════════════════════════════════════════════╗
║                   P2P Chat — Setup                       ║
║                                                          ║
║  LAN address: 192.168.1.5:45231  (share with peers)     ║
║                                                          ║
║  Node ID  (unique identifier, no spaces):               ║
║  ┌─────────────────────────────────────────────────┐    ║
║  │ alice_                                           │    ║
║  └─────────────────────────────────────────────────┘    ║
║                                                          ║
║  Username  (display name visible to peers):             ║
║  ┌─────────────────────────────────────────────────┐    ║
║  │                                                  │    ║
║  └─────────────────────────────────────────────────┘    ║
║                                                          ║
║  Tab / ↓ / Enter → next field   |   Ctrl-C to quit      ║
╚══════════════════════════════════════════════════════════╝
```

填写完毕按 Enter 即进入聊天界面。Setup 界面同时显示本机地址（`127.0.0.1:PORT`）和局域网地址（`LAN_IP:PORT`），**将 LAN 地址告诉对方**，对方即可连接。

---

## 可选命令行参数

不需要任何参数即可启动。如有需要，可以指定：

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `-P` / `--port` | `0`（自动） | 监听端口。`0` = 依次尝试 9000-9004，绑定第一个空闲端口 |
| `--peers` | 空 | 启动后自动连接的节点（格式：`ip:port:node_id`，多个用空格分隔） |
| `--peers-file` | `known_peers.json` | 节点注册表文件路径 |

示例：

```bash
# 强制使用端口 8888（例如默认范围全被占用）
cargo run --release -- --port 8888

# 启动时自动连接 Alice
cargo run --release -- --peers "192.168.1.5:9000:alice"
```

---

## TUI 界面

### Setup 界面（启动时）

```
╔═══════════════════════════════╗
║    P2P Chat — Setup           ║
║                               ║
║  LAN address: 192.168.1.5:PORT║
║                               ║
║  Node ID:   [ alice        ]  ║
║  Username:  [ Alice        ]  ║
║                               ║
║  Enter: 确认  Tab: 切换字段   ║
╚═══════════════════════════════╝
```

**Setup 键盘操作：**

| 键 | 功能 |
|---|------|
| `Enter` | 确认当前字段 / 进入下一字段 / 完成设置 |
| `Tab` / `↓` / `↑` | 在两个字段间切换 |
| `Backspace` | 删除字符 |
| `Ctrl-C` | 退出程序 |

> Node ID 不允许包含空格。

### 聊天界面

```
┌─ P2P Chat | Online: alice, bob ──────────────────── ↑3 ─┐
│ [SYSTEM] alice (Alice) connected                         │
│ [14:32:05] Alice (alice): Hello!                         │  ← 青色：收到消息
│ [14:32:10] You  (bob) → alice: Hi!                       │  ← 绿色：发出消息
│ [SYSTEM] charlie disconnected: connection closed         │  ← 黄色：系统消息
├──────────────────────────────────────────────────────────┤
│ [alice]> _                                               │  ← 聊天模式输入栏
└──────────────────────────────────────────────────────────┘
```

**聊天界面键盘操作：**

| 键 | 功能 |
|---|------|
| `Enter` | 提交命令或发送消息 |
| `Ctrl-C` | 优雅退出 |
| `↑ / ↓`（输入为空时） | 向上/下滚动消息 |
| `↑ / ↓`（有输入时） | 命令历史导航 |
| `PageUp / PageDown` | 滚动 ±10 行 |
| `Esc` | 回到最新消息（底部） |
| `Backspace` | 删除最后一个字符 |

---

## 交互命令

| 命令 | 说明 |
|------|------|
| `connect <ip>` | 按 IP 连接（并发探测 9000-9004 端口，node_id 从握手获取） |
| `connect <ip:port>` | 直接指定地址连接（node_id 从握手获取，无需手动填写） |
| `connect <node_id>` | 连接已知节点（从注册表查地址，自动选择连接方式） |
| `chat <node_id>` | 进入专注聊天模式（节点离线时自动重连） |
| `chat` | 退出聊天模式 |
| `list` | 查看当前已连接的节点 |
| `peers` | 查看注册表中所有已知节点（含在线状态） |
| `send <node_id> <message>` | 发送私聊消息（节点离线时自动重连） |
| `broadcast <message>` | 广播消息给所有已连接节点 |
| `help` | 显示帮助信息 |
| `quit` | 退出程序 |

### 聊天模式

`chat <node_id>` 进入专注对话，后续直接输入内容即可发送：

```
> chat alice
[SYSTEM] Entering chat with 'alice'. Type to send, /exit to leave.

[alice]> Hello!
[14:32:05] You (bob) → alice: Hello!

[alice]> /list          ← 以 / 开头运行命令
Connected peers:
  - alice

[alice]> /exit          ← 退出聊天模式
Left chat with 'alice'
>
```

### 消息显示格式

| 场景 | 显示 |
|------|------|
| 收到私信 | `[14:32:05] Alice (alice): Hello!` |
| 收到广播 | `[14:32:05] Alice (alice) → all: Hi!` |
| 发送私信 | `[14:32:05] You (bob) → alice: Hello!` |
| 发送广播 | `[14:32:05] You (bob) → all [2/2]: Hi!` |

---

## 典型使用流程

### 两节点首次连接

```
# 终端 1 — Alice 启动（自动绑定 9000）
$ cargo run --release
> 填写: Node ID = alice, Username = Alice
> 进入聊天界面，看到: "Listening → local: 127.0.0.1:9000   LAN: 192.168.1.5:9000"

# 终端 2 — Bob 启动（自动绑定 9000，若被占则绑定 9001）
$ cargo run --release
> 填写: Node ID = bob, Username = Bob
> 进入聊天界面后，只需输入对方 IP（无需知道端口）：
> connect 192.168.1.5
[SYSTEM] alice (Alice) connected

# 也可以直接指定端口（不需要 node_id，从握手自动获取）：
> connect 192.168.1.5:9000
[SYSTEM] alice (Alice) connected

# 此后双方均记录了对方信息，下次重启后直接:
> connect alice
```

### 三节点聊天

```
# 每个节点直接 cargo run，填好信息后：
# Charlie 连接到 Alice 和 Bob（只需 IP，无需知道端口）：
> connect 192.168.1.5
> connect 192.168.1.6

# 广播给所有人：
> broadcast Good morning everyone!

# 与 Alice 私聊：
> chat alice
[alice]> Hey Alice, how are you?
```

---

## 端口策略

启动时按以下优先级绑定端口：

1. **`--port <N>` 指定了端口** → 直接绑定，失败则报错退出
2. **未指定端口** → 依次尝试默认端口 `9000 → 9001 → 9002 → 9003 → 9004`，绑定第一个空闲端口；全部被占用则报错，需手动指定 `--port`

由于绑定的是固定的知名端口，对端只需知道 IP 地址即可连接：`connect 192.168.1.5` 会并发探测全部默认端口，自动找到正在运行的实例。

> 大多数情况下单台机器只会运行一个实例，直接绑定 `9000`；如需同时运行多个实例或 `9000` 被占用，程序自动使用后续备用端口。

---

## 节点注册表

每次成功握手后，双方节点互相记录对方的监听地址到 `known_peers.json`：

```json
[
  {
    "node_id": "alice",
    "username": "Alice",
    "address": "192.168.1.5"
  },
  {
    "node_id": "custom",
    "username": "Custom",
    "address": "192.168.1.6:8888"
  }
]
```

- **默认端口（9000-9004）** → 只记录 IP，下次 `connect alice` 自动探测端口
- **非默认端口（--port 指定）** → 记录完整 `ip:port`，保证精确重连
- 记录的是**监听地址**，不是临时 TCP 源端口
- 双向握手确保连接方和被连接方各自独立完成更新

---

## 架构概览

```
src/
├── main.rs        CLI 参数 + 绑定端口 + 调用 setup + 启动 TUI    (~70 行)
├── event.rs       AppEvent / NodeCommand / DisplayLine            (~70 行)
├── ui.rs          Setup TUI + 聊天 TUI (ratatui + crossterm)      (~500 行)
├── node.rs        P2PNode 状态机，处理 NodeCommand 循环           (~420 行)
├── network.rs     handle_* 循环 + format_time + now_secs          (~170 行)
├── connection.rs  ConnectionMap / 帧编解码                        (~80 行)
├── message.rs     P2PMessage 协议枚举                             (~120 行)
└── registry.rs    PeerRegistry / KnownPeer / parse_peer_arg      (~100 行)
```

**启动流程：**

```
main()
  │
  ├─ TcpListener::bind("0.0.0.0:0")   ← OS 分配端口
  │
  ├─ run_setup(listen_addr)            ← TUI 收集 node_id / username
  │    └─ 返回 (node_id, username, Terminal)   ← Terminal 保持 raw mode
  │
  ├─ P2PNode::new(...)
  ├─ tokio::spawn → node.start(cmd_rx, listener)
  │
  └─ run_tui(terminal, ...)            ← 复用同一 Terminal，无闪烁
```

---

## 开发调试

```bash
# 编译检查
cargo check

# 开发构建
cargo build

# 发布构建（推荐）
cargo build --release

# 查看运行日志（不影响 TUI）
tail -f p2p-chat.log
```

---

## 故障排除

**连接失败**：确认对方节点正在运行，地址和端口正确，防火墙未阻挡

**注册表地址过期**：对端换了端口？用 `connect <新ip:port> <node_id>` 重连，注册表自动更新

**消息发送失败**：用 `list` 确认在线状态；若节点在注册表中但离线，`send` 会自动尝试重连

---

## 可能的扩展方向

| 方向 | 说明 |
|------|------|
| 网络发现 | UDP 广播自动发现局域网节点 |
| Keepalive 检测 | 定时探活，超时标记离线 |
| TLS 加密 | tokio-rustls 防中间人攻击 |
| 群组功能 | Group 消息类型 + 成员管理 |
| 文件传输 | 分块传输 + 断点续传 |

---

## 许可证

MIT License

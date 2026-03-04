# netscope

> CDN 测速 & 连通性探测工具 — 支持 Apple / Cloudflare 后端，交互式 TUI，多路径 & 分流检测。

[![Release](https://img.shields.io/github/v/release/xjoker/netscope)](https://github.com/xjoker/netscope/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

[English](README.md)

![TUI 截图](docs/images/screenshot-20260302-192112.jpg)

---

## 功能特性

- **交互式 TUI** — 实时进度显示、延迟 sparkline 折线图、下载阶段柱状图
- **双后端** — Apple CDN（`mensura.cdn-apple.com`，针对中国大陆路由优化）；Cloudflare（`speed.cloudflare.com`，适合全球测速）
- **多路径测速** — 并发测试 v4-CN / v4-Global / v6-CN / v6-Global 四条路径，自动检测分流路由
- **连通性探测** — 检测 60+ 个站点，覆盖 11 个分类（AI、社交、流媒体、搜索、新闻、游戏、开发、云服务、加密货币、NSFW、国内），显示 TTFB 延迟与归属地
- **出口 IP 检测** — 解析公网 IPv4 & IPv6 及地理位置，CN ≠ Global 时自动告警分流
- **JSON 输出** — 结构化机读结果，稳定 Schema（`schema_version: 1`），支持 `--verbose` 输出详细信息
- **代理支持** — `http` / `https` / `socks5` / `socks5h`
- **单文件静态二进制** — 零运行时依赖

---

## 安装

### 一键运行

**Linux x86_64**
```bash
curl -fsSL https://github.com/xjoker/netscope/releases/latest/download/netscope-linux-x86_64 -o netscope && chmod +x netscope && ./netscope
```

**macOS Apple Silicon（M1/M2/M3）**
```bash
curl -fsSL https://github.com/xjoker/netscope/releases/latest/download/netscope-macos-aarch64 -o netscope && chmod +x netscope && ./netscope
```

**Windows PowerShell**
```powershell
irm https://github.com/xjoker/netscope/releases/latest/download/netscope-windows-x86_64.exe -OutFile netscope.exe; .\netscope.exe
```

### 安装到系统

```bash
# macOS / Linux — 将 <BINARY> 替换为下表中的文件名
curl -fsSL https://github.com/xjoker/netscope/releases/latest/download/<BINARY> -o netscope && chmod +x netscope && sudo mv netscope /usr/local/bin/
```

| 平台 | 文件名 |
|------|--------|
| macOS Apple Silicon | `netscope-macos-aarch64` |
| macOS Intel | `netscope-macos-x86_64` |
| Linux x86_64 | `netscope-linux-x86_64` |
| Linux aarch64 | `netscope-linux-aarch64` |
| Windows x86_64 | `netscope-windows-x86_64.exe` |

### 中国大陆加速下载

> GitHub 访问较慢时，将命令中的 `https://github.com` 替换为 `https://gh.felicity.ac.cn/https://github.com` 即可。

### 其他方式

- 浏览所有版本：[github.com/xjoker/netscope/releases](https://github.com/xjoker/netscope/releases)
- 从源码构建：`cargo build --release`（二进制位于 `target/release/netscope`）

---

## 快速上手

```bash
# 完整测速（TUI 模式，Apple 后端）
netscope

# 使用 Cloudflare 后端测速
netscope --backend cloudflare

# 强制走中国大陆路由（ECS 提示）
netscope --country CN

# 仅做连通性探测
netscope probe

# JSON 输出（适合管道 / CI）
netscope --json | jq .

# 详细 JSON 输出（含各路径详情）
netscope --json --verbose | jq .

# 通过代理测速
netscope --proxy socks5://127.0.0.1:1080
```

---

## 命令说明

```
netscope [选项] [子命令]

子命令:
  ping      仅测延迟（HTTP RTT + TCP 建连时间）
  download  仅测下载速度
  upload    仅测上传速度
  full      完整测速：延迟 + 下载 + 上传 + 连通性探测  [默认]
  probe     连通性探测（不测速）

选项:
      --backend <BACKEND>    apple | cloudflare  [默认: apple]
      --country <CC>         强制路由国家（如 CN、HK、SG、US）
      --proxy <URL>          代理 URL（http/https/socks5/socks5h）
      --timeout <SECS>       单次请求超时秒数  [默认: 8]
      --json                 结果以 JSON 输出到 stdout（进度信息静默）
      --verbose              输出各路径详细候选信息（需配合 --json）
```

### 子命令参数

```
netscope ping     [--count <次数>]                          # 默认: 8 次
netscope download [--duration <秒>]                         # 默认: 20 秒
netscope upload   [--ul-mib <MiB>] [--ul-repeat <次>]      # 默认: 16 MiB × 3
netscope full     [--count <次>] [--duration <秒>] [--ul-mib <MiB>] [--ul-repeat <次>]

netscope probe    [--concurrency <并发数>]      # 默认: 6
                  [--probe-timeout <秒>]         # 默认: 10
                  [--category <分类,...>]         # ai,social,streaming,...
                  [--site <关键词>]               # 按名称过滤
                  [--skip-geo]                    # 跳过 GeoIP 查询
```

---

## TUI 界面说明

### 顶部徽章

| 徽章 | 含义 |
|------|------|
| `APPLE` / `CLOUDFLARE` | 当前使用的测速后端 |
| `CN Mode` | 出口 IP 检测为中国大陆，将同时测试 CN 和 Global 路径 |
| `Global` | 非大陆出口，仅测试 Global 路径 |

顶部同时显示出口 IP 及归属地。当 CN 侧出口与 Global 侧出口不同时，会出现 **⚠ split routing** 警告，通常表示代理或 VPN 仅对部分流量生效。

### 测速结果面板（Speed Results）

| 列 | 含义 |
|----|------|
| Path | 路径标识：`v4-cn` / `v4-global` / `v6-cn` / `v6-global`（IP 协议 + 路由方向） |
| CDN Node | 本次测速所选 CDN 节点的 IP 及归属地 |
| HTTP-RTT | 跨多次 ping 采样的 HTTP 往返时延中位数 |
| TCP-RTT | TCP 建连时延（使用代理时不可用） |
| Download | 多流并发各阶段中的最佳下载速度 |
| Upload | 上传速度中位数 |

每条路径下方的子行展示延迟 sparkline 折线图、各阶段下载柱状图及统计信息。

### 连通性面板（Connectivity）

`●`（实心）= 可达，`○`（空心）= 不可达 / 超时。数字为 TTFB 首字节时间（毫秒）。归属地来自 `cdn-cgi/trace` 或 GeoIP 兜底。

### 快捷键

| 按键 | 功能 |
|------|------|
| `q` / `Q` / `Esc` | 退出 |
| `Tab` | 在测速 / 连通性面板间切换焦点 |
| `↑` / `k` | 向上滚动 |
| `↓` / `j` | 向下滚动 |

---

## 连通性探测分类

| 分类 | 站点 |
|------|------|
| AI | ChatGPT、OpenAI、Claude、Gemini、Grok、Perplexity、Copilot、Midjourney |
| 社交 | Twitter、Instagram、Facebook、Threads、Reddit、Discord、Telegram、Medium、Quora、LinkedIn、小红书 |
| 流媒体 | YouTube、Netflix、Spotify、Twitch、Crunchyroll、AbemaTV、TikTok、Disney+ |
| 搜索 | Google、Bing、DuckDuckGo、Brave |
| 新闻 | Wikipedia、BBC、Reuters、NYT |
| 游戏 | Steam、Epic、Battle.net、PlayStation、Xbox |
| 开发 | GitHub、GitLab、Cloudflare、NPM、Docker Hub、PyPI |
| 云服务 | AWS、GCP、Azure、Vercel、Render |
| 加密货币 | Binance、Coinbase、OKX、Bybit、Gate.io |
| NSFW | E-Hentai、MissAV、JAVDB、Hanime1、91Porn、Haijiao |
| 国内 | QQ、微信、哔哩哔哩、微博、百度、淘宝、京东、小米、IPIP、字节跳动、网易 |

连通性面板中的归属地来自 `cdn-cgi/trace`（Cloudflare 边缘节点）或 GeoIP 兜底。延迟（ms）为 TTFB 首字节时间。

---

## JSON 输出格式

```jsonc
{
  "schema_version": 1,
  "mode": "full",
  "backend": "apple",
  "ts": 1700000000,
  "egress_v4_cn": "1.2.3.4",
  "egress_consistent": true,
  "resolver_country": "CN",
  "download_mbps": 523.4,
  "upload_mbps": 87.1,
  "rtt_ms": 12.3,
  "paths": [
    {
      "path_id": "v4-cn",
      "cdn_ip": "17.253.x.x",
      "cdn_location": "China/Shanghai (Chinanet)",
      "download_mbps": 651.2,
      "upload_mbps": 91.4,
      "rtt_ms": 8.1
    }
  ]
}
```

`schema_version` 固定为 `1`，后续版本**只增不删**，已有字段不会被移除或改变类型。

---

## 退出码

| 退出码 | 含义 |
|--------|------|
| `0` | 全部测试通过 |
| `2` | 致命错误 / 所有路径失败 |
| `3` | 部分失败（某些路径失败） |
| `130` | 用户中断（Ctrl-C / `q`） |

---

## 常见问题

**macOS：提示"无法验证开发者"**

这是 macOS Gatekeeper 拦截未签名二进制的正常行为。运行一次以下命令清除隔离标记：
```bash
xattr -d com.apple.quarantine ./netscope
```
或：在 Finder 中右键点击二进制文件 → 打开 → 打开。

**Windows：TUI 显示乱码 / 方块**

TUI 使用了 Unicode 方块字符和盲文点阵字符。请使用 [Windows Terminal](https://aka.ms/terminal) 并选择支持这些字符的字体（如 Cascadia Code、Nerd Fonts）。内置的 CMD 和旧版 PowerShell 控制台无法正确渲染。

**`probe` 探测时大量超时**

在中国大陆运行时属于正常现象——大多数境外站点被屏蔽或严重限速。可适当增大 `--probe-timeout`：
```bash
netscope probe --probe-timeout 15
```

**Apple 与 Cloudflare 后端的速度差异很大**

两者测量的是不同的东西：Apple 后端使用基于 DoH 的 IP 选择和 IP 固定，针对所在位置优化反映 Apple 服务的性能；Cloudflare 后端测量到最近 Cloudflare PoP 节点的吞吐量。两者都不是通用 ISP 速度测试工具。

---

## 开源协议

MIT

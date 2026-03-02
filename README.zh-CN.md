# netscope

> CDN 测速 & 连通性探测工具 — 支持 Apple / Cloudflare 后端，交互式 TUI，多路径 & 分流检测。

[![Release](https://img.shields.io/github/v/release/xjoker/netscope)](https://github.com/xjoker/netscope/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

[English](README.md)

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

### 临时运行（不安装）

下载后直接运行，不写入系统，用完即弃。

**macOS — Apple Silicon（M1/M2/M3）**
```bash
curl -fsSL https://github.com/xjoker/netscope/releases/latest/download/netscope-macos-aarch64.tar.gz | tar -xz && ./netscope
```

**macOS — Intel**
```bash
curl -fsSL https://github.com/xjoker/netscope/releases/latest/download/netscope-macos-x86_64.tar.gz | tar -xz && ./netscope
```

**Linux — x86_64**
```bash
curl -fsSL https://github.com/xjoker/netscope/releases/latest/download/netscope-linux-x86_64.tar.gz | tar -xz && ./netscope
```

**Linux — aarch64**
```bash
curl -fsSL https://github.com/xjoker/netscope/releases/latest/download/netscope-linux-aarch64.tar.gz | tar -xz && ./netscope
```

**Windows — PowerShell**
```powershell
irm https://github.com/xjoker/netscope/releases/latest/download/netscope-windows-x86_64.zip -OutFile netscope.zip; Expand-Archive netscope.zip .; .\netscope.exe
```

### 安装到系统

安装后可在任意终端直接使用 `netscope`。

**macOS — Apple Silicon（M1/M2/M3）**
```bash
curl -fsSL https://github.com/xjoker/netscope/releases/latest/download/netscope-macos-aarch64.tar.gz | tar -xz && sudo mv netscope /usr/local/bin/
```

**macOS — Intel**
```bash
curl -fsSL https://github.com/xjoker/netscope/releases/latest/download/netscope-macos-x86_64.tar.gz | tar -xz && sudo mv netscope /usr/local/bin/
```

**Linux — x86_64**
```bash
curl -fsSL https://github.com/xjoker/netscope/releases/latest/download/netscope-linux-x86_64.tar.gz | tar -xz && sudo mv netscope /usr/local/bin/
```

**Linux — aarch64**
```bash
curl -fsSL https://github.com/xjoker/netscope/releases/latest/download/netscope-linux-aarch64.tar.gz | tar -xz && sudo mv netscope /usr/local/bin/
```

**Windows — PowerShell（安装到 `%ProgramFiles%\netscope`）**
```powershell
irm https://github.com/xjoker/netscope/releases/latest/download/netscope-windows-x86_64.zip -OutFile netscope.zip
Expand-Archive netscope.zip -DestinationPath "$env:ProgramFiles\netscope"
[Environment]::SetEnvironmentVariable("PATH", $env:PATH + ";$env:ProgramFiles\netscope", "User")
```

### 下载历史版本

浏览所有版本：[github.com/xjoker/netscope/releases](https://github.com/xjoker/netscope/releases)

### 从源码构建

```bash
cargo build --release
# 二进制位于：target/release/netscope
```

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
  ping      仅测延迟
  download  仅测下载速度
  upload    仅测上传速度
  full      完整测速：延迟 + 下载 + 上传  [默认]
  probe     连通性探测（不测速）

选项:
      --backend <后端>      apple | cloudflare  [默认: apple]
      --country <国家码>    强制路由国家（如 CN）
      --proxy <代理地址>    代理 URL（http/https/socks5/socks5h）
      --timeout <秒>        单次请求超时秒数  [默认: 8]
      --json                结果以 JSON 输出到 stdout
      --verbose             输出各阶段详细信息
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

## TUI 快捷键

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
| `1` | 部分失败（某些路径失败） |
| `2` | 致命错误 |
| `130` | 用户中断（Ctrl-C / `q`） |

---

## 开源协议

MIT

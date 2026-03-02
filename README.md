# netscope

> CDN speed test & connectivity probe — Apple / Cloudflare backends, interactive TUI, multi-path & split-routing detection.

[![Release](https://img.shields.io/github/v/release/xjoker/netscope)](https://github.com/xjoker/netscope/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

[中文说明](README.zh-CN.md)

---

## Features

- **Interactive TUI** — real-time progress, latency sparklines, download stage bar charts
- **Dual backend** — Apple CDN (`mensura.cdn-apple.com`) optimised for China routing; Cloudflare (`speed.cloudflare.com`) for global
- **Multi-path testing** — concurrently tests v4-CN / v4-Global / v6-CN / v6-Global paths and detects split routing
- **Connectivity probe** — checks 60+ sites across 11 categories (AI, Social, Streaming, Search, News, Game, Dev, Cloud, Crypto, NSFW, CN) with TTFB and country-code display
- **Egress detection** — resolves your public IPv4 & IPv6 with geolocation; warns on CN ≠ Global mismatch
- **JSON output** — machine-readable results with stable schema (`schema_version: 1`), verbose flag for full details
- **Proxy support** — `http` / `https` / `socks5` / `socks5h`
- **Single static binary** — zero runtime dependencies

---

## Installation

### One-line install (latest release)

**macOS — Apple Silicon (M1/M2/M3)**
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

**Windows — PowerShell**
```powershell
irm https://github.com/xjoker/netscope/releases/latest/download/netscope-windows-x86_64.zip -OutFile netscope.zip; Expand-Archive netscope.zip .; .\netscope.exe
```

After install, run `netscope` directly from any terminal.

### Download versioned release

Browse all releases at [github.com/xjoker/netscope/releases](https://github.com/xjoker/netscope/releases).

### Build from source

```bash
cargo build --release
# binary at: target/release/netscope
```

---

## Quick Start

```bash
# Full speed test (TUI, Apple backend)
netscope

# Full speed test via Cloudflare
netscope --backend cloudflare

# Force CN routing (ECS hints for Chinese DNS resolvers)
netscope --country CN

# Connectivity probe only
netscope probe

# JSON output (piped / CI)
netscope --json | jq .

# Verbose JSON with per-path details
netscope --json --verbose | jq .

# Through a proxy
netscope --proxy socks5://127.0.0.1:1080
```

---

## Commands

```
netscope [OPTIONS] [COMMAND]

Commands:
  ping      Latency test only
  download  Download speed test only
  upload    Upload speed test only
  full      Full test: ping + download + upload  [default]
  probe     Connectivity probe (no speed test)

Options:
      --backend <BACKEND>    apple | cloudflare  [default: apple]
      --country <COUNTRY>    Force routing country code (e.g. CN)
      --proxy <PROXY>        Proxy URL (http/https/socks5/socks5h)
      --timeout <TIMEOUT>    Per-request timeout in seconds  [default: 8]
      --json                 Output JSON results to stdout
      --verbose              Include per-stage details in output
```

### Subcommand options

```
netscope ping     [--count <N>]                         # default: 8 samples
netscope download [--duration <SECS>]                   # default: 20s
netscope upload   [--ul-mib <MiB>] [--ul-repeat <N>]   # default: 16 MiB × 3
netscope full     [--count <N>] [--duration <SECS>] [--ul-mib <MiB>] [--ul-repeat <N>]

netscope probe    [--concurrency <N>]         # default: 6
                  [--probe-timeout <SECS>]     # default: 10
                  [--category <cat,...>]        # ai,social,streaming,...
                  [--site <keyword>]            # filter by name
                  [--skip-geo]                  # skip GeoIP lookup
```

---

## TUI Key Bindings

| Key | Action |
|-----|--------|
| `q` / `Q` / `Esc` | Quit |
| `Tab` | Switch focus between Speed / Connectivity panels |
| `↑` / `k` | Scroll up |
| `↓` / `j` | Scroll down |

---

## Connectivity Probe Categories

| Category | Sites |
|----------|-------|
| AI | ChatGPT, OpenAI, Claude, Gemini, Grok, Perplexity, Copilot, Midjourney |
| Social | Twitter, Instagram, Facebook, Threads, Reddit, Discord, Telegram, Medium, Quora, LinkedIn, XiaoHongShu |
| Streaming | YouTube, Netflix, Spotify, Twitch, Crunchyroll, AbemaTV, TikTok, Disney+ |
| Search | Google, Bing, DuckDuckGo, Brave |
| News | Wikipedia, BBC, Reuters, NYT |
| Game | Steam, Epic, Battle.net, PlayStation, Xbox |
| Dev | GitHub, GitLab, Cloudflare, NPM, Docker Hub, PyPI |
| Cloud | AWS, GCP, Azure, Vercel, Render |
| Crypto | Binance, Coinbase, OKX, Bybit, Gate.io |
| NSFW | E-Hentai, MissAV, JAVDB, Hanime1, 91Porn, Haijiao |
| CN | QQ, WeChat, Bilibili, Weibo, Baidu, Taobao, JD, Xiaomi, IPIP, ByteDance, NetEase |

Country codes in the Connectivity panel come from `cdn-cgi/trace` (Cloudflare edge) or GeoIP fallback. Latency (ms) is TTFB.

---

## JSON Output

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

`schema_version` is `1` and is **additive-only** — existing fields will never be removed or type-changed in future releases.

---

## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | All tests passed |
| `1` | Partial failure (some paths failed) |
| `2` | Fatal error |
| `130` | Aborted by user (Ctrl-C / `q`) |

---

## License

MIT
